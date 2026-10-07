//! Journaled source and executable transitions. Git children have bounded process
//! owners; only the final checked filesystem rename is a synchronous commit point.
use crate::harness_activation::{
    copy_binary, executable_metadata, signature, smoke_selected, PreparedArtifacts,
};
use crate::harness_activity::HarnessActivity;
use crate::harness_engineering::{artifact_digest, source_snapshot};
use crate::harness_source_checkout::{audit_candidate, run_git, SourceCheckout};
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{
    canonical_digest, evaluate_harness, load_harness_cycle, update_harness_cycle, HarnessPlan,
    HarnessStage, HarnessVerdict, LearningVault, SourceRepositorySnapshotV1,
};
use serde::{Deserialize, Serialize};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Prepared,
    SourceAdopting,
    SourceAdopted,
    Replacing,
    Active,
    Restoring,
    BinaryRestored,
    Reverting,
    RolledBack,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Promotion {
    pub plan_digest: String,
    pub repository: PathBuf,
    pub artifacts: PreparedArtifacts,
    pub candidate_source: SourceRepositorySnapshotV1,
    pub phase: Phase,
    pub reverted_head: Option<String>,
    pub work: Option<(String, u64)>,
}
fn path(id: &str) -> String {
    format!("harness/source/{id}-promotion.json")
}
pub fn report(vault: &LearningVault, id: &str) -> Result<Option<Promotion>> {
    let (plan, cycle) = load_harness_cycle(vault, id)?;
    let journal = vault.read_mutable::<Promotion>(&path(id))?;
    if let Some(journal) = &journal {
        validate(journal, &plan)?;
        if cycle.candidate_digest.as_deref() != Some(&journal.artifacts.candidate_digest)
            || cycle.candidate_commit.as_deref() != Some(&journal.candidate_source.head)
        {
            bail!("promotion candidate identity changed");
        }
    }
    Ok(journal)
}
fn validate(journal: &Promotion, plan: &HarnessPlan) -> Result<()> {
    let artifacts = &journal.artifacts;
    if journal.plan_digest != plan.digest
        || artifacts.plan_digest != plan.digest
        || !artifacts.ready
        || artifacts.signed_digest.is_none()
        || Some(&artifacts.destination) != plan.activation_executable.as_ref()
        || artifacts.baseline_digest != plan.baseline_binary_digest
        || artifacts.baseline_smoke.len() != 2
        || artifacts.candidate_smoke.len() != 2
    {
        bail!("promotion journal does not match the frozen plan");
    }
    crate::harness_activation::validate_binding(
        artifacts,
        plan,
        &artifacts.candidate_digest,
        &artifacts.trials_digest,
    )?;
    Ok(())
}
fn save(vault: &LearningVault, id: &str, journal: &Promotion) -> Result<()> {
    vault.write_mutable(&path(id), journal).map(|_| ())
}
fn running(vault: &LearningVault, id: &str) -> Result<()> {
    if load_harness_cycle(vault, id)?.1.stage == HarnessStage::Paused {
        bail!("promotion paused");
    }
    Ok(())
}
fn stage(vault: &LearningVault, id: &str, next: HarnessStage) -> Result<()> {
    update_harness_cycle(vault, id, |state| {
        if state.stage == HarnessStage::Paused {
            state.resume_stage = Some(next);
        } else {
            state.stage = next;
        }
        Ok(())
    })
}
async fn git(repository: &Path, temporary: &Path, args: &[&str]) -> Result<String> {
    Ok(
        run_git(repository, temporary, args.iter().map(Into::into).collect())
            .await?
            .trim()
            .into(),
    )
}
fn at_candidate(
    current: &SourceRepositorySnapshotV1,
    plan: &HarnessPlan,
    candidate: &SourceRepositorySnapshotV1,
) -> bool {
    current.branch == plan.baseline.branch
        && current.head == candidate.head
        && current.tracked_tree_digest == candidate.tracked_tree_digest
        && current.submodule_digest == candidate.submodule_digest
}

fn publication_lease_root(vault: &LearningVault) -> Result<PathBuf> {
    match ca_automation::runtime::current_run_store() {
        Some(store) => Ok(store.context().lease_root.clone()),
        None => crate::harness_automation::compatibility_lease_root(vault),
    }
}

async fn publish_source_candidate(
    vault: &LearningVault,
    id: &str,
    plan: &HarnessPlan,
    journal: &Promotion,
    checkout: &SourceCheckout,
    temporary: &Path,
    repository_lease: Option<&ca_automation::runtime::ResourceLease>,
) -> Result<()> {
    let binding = SourcePublicationBinding {
        transition: "activation",
        run_id: format!("harness-{}", canonical_digest(&(id, &plan.digest))?),
        base_commit: plan.baseline.head.clone(),
        candidate_commit: journal.candidate_source.head.clone(),
        retained_root: checkout.workspace.canonicalize()?,
    };
    Box::pin(publish_source_transition(
        vault,
        id,
        plan,
        journal,
        &binding,
        temporary,
        repository_lease,
    ))
    .await
}

struct SourcePublicationBinding {
    transition: &'static str,
    run_id: String,
    base_commit: String,
    candidate_commit: String,
    retained_root: PathBuf,
}

fn rollback_publication_id(id: &str, plan: &HarnessPlan) -> Result<String> {
    Ok(format!(
        "harness-rollback-{}",
        canonical_digest(&(id, &plan.digest))?
    ))
}

async fn publish_source_rollback(
    vault: &LearningVault,
    id: &str,
    plan: &HarnessPlan,
    journal: &Promotion,
    temporary: &Path,
    repository_lease: Option<&ca_automation::runtime::ResourceLease>,
) -> Result<()> {
    let store = ca_automation::repository::RepositoryPublicationStore::new(
        vault.resolve_relative("harness/source-publications")?,
    );
    let run_id = rollback_publication_id(id, plan)?;
    let existing = store.load(&run_id)?;
    let (candidate_commit, retained_root) = if let Some(existing) = existing {
        (
            existing.acceptance.candidate.commit,
            existing.acceptance.candidate.worktree,
        )
    } else {
        let relative = format!("harness/source-rollbacks/{run_id}");
        for directory in ["harness", "harness/source-rollbacks", &relative] {
            ca_automation::runtime::private_dir(&vault.resolve_relative(directory)?)?;
        }
        let retained_root = vault.resolve_relative(&relative)?.canonicalize()?;
        let tree = git(
            &journal.repository,
            temporary,
            &["rev-parse", &format!("{}^{{tree}}", plan.baseline.head)],
        )
        .await?;
        let message = format!(
            "Revert harness candidate {}\n\nThis reverts commit {}.\n",
            journal.candidate_source.head, journal.candidate_source.head
        );
        // This writes only an immutable Git object. Working files, HEAD and
        // branch refs stay untouched until guarded publication pins them.
        let candidate_commit = git(
            &journal.repository,
            temporary,
            &[
                "commit-tree",
                &tree,
                "-p",
                &journal.candidate_source.head,
                "-m",
                &message,
            ],
        )
        .await?;
        (candidate_commit, retained_root)
    };
    let binding = SourcePublicationBinding {
        transition: "rollback",
        run_id,
        base_commit: journal.candidate_source.head.clone(),
        candidate_commit,
        retained_root,
    };
    Box::pin(publish_source_transition(
        vault,
        id,
        plan,
        journal,
        &binding,
        temporary,
        repository_lease,
    ))
    .await
}

async fn publish_source_transition(
    vault: &LearningVault,
    id: &str,
    plan: &HarnessPlan,
    journal: &Promotion,
    binding: &SourcePublicationBinding,
    temporary: &Path,
    repository_lease: Option<&ca_automation::runtime::ResourceLease>,
) -> Result<()> {
    use ca_automation::repository::{
        RepositoryAcceptance, RepositoryAgentRoute, RepositoryCandidate, RepositoryPricing,
        RepositoryPublication, RepositoryPublicationState, RepositoryPublicationStore,
        RepositoryReviewReceipt, RepositorySnapshot,
    };
    let repository = &journal.repository;
    let target_ref = format!("refs/heads/{}", plan.baseline.branch);
    let common_dir = PathBuf::from(
        git(
            repository,
            temporary,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .await?,
    )
    .canonicalize()?;
    let candidate = &binding.candidate_commit;
    let tree = git(
        repository,
        temporary,
        &["rev-parse", &format!("{candidate}^{{tree}}")],
    )
    .await?;
    let paths = run_git(
        repository,
        temporary,
        [
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            &binding.base_commit,
            candidate,
            "--",
        ]
        .into_iter()
        .map(Into::into)
        .collect(),
    )
    .await?;
    let changed_paths: Vec<_> = paths
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect();
    // This immutable binding also preserves already admitted V1 activations:
    // their durable promotion transaction is their recovery authority.
    let authority =
        canonical_digest(&(&plan.digest, &journal.artifacts, &journal.candidate_source))?;
    let publication = RepositoryPublication {
        version: 1,
        run_id: binding.run_id.clone(),
        snapshot: RepositorySnapshot {
            repository: repository.clone(),
            common_dir,
            target_ref,
            base_commit: binding.base_commit.clone(),
            task_sha256: authority.clone(),
        },
        acceptance: RepositoryAcceptance {
            candidate: RepositoryCandidate {
                worktree: binding.retained_root.clone(),
                commit: candidate.clone(),
                tree: tree.clone(),
                changed_paths,
            },
            checks: Vec::new(),
            review: RepositoryReviewReceipt {
                route: RepositoryAgentRoute {
                    provider: "ca".into(),
                    model: "harness-promotion-review".into(),
                    pricing: RepositoryPricing::LocalZero,
                },
                candidate_tree: tree,
                passed: true,
                report: serde_json::json!({
                    "schema":"ca.harness-publication-authorization.v1",
                    "cycle_id":id,
                    "transition":binding.transition,
                    "plan_digest":plan.digest,
                    "candidate_digest":journal.artifacts.candidate_digest,
                    "evaluation_digest":journal.artifacts.trials_digest,
                    "promotion_authority_digest":authority,
                }),
            },
        },
        state: RepositoryPublicationState::Prepared,
        summary: "Reviewed harness candidate retained for guarded source activation".into(),
        checkout: None,
    };
    let store =
        RepositoryPublicationStore::new(vault.resolve_relative("harness/source-publications")?);
    let lease_root = publication_lease_root(vault)?;
    let publication = ca_automation::repository::CONTAINED_COMMANDS
        .scope(
            crate::jobs::worker_contained(),
            Box::pin(ca_automation::repository::publish_authorized_candidate(
                publication,
                &store,
                &lease_root,
                repository_lease,
            )),
        )
        .await?;
    if publication.state != RepositoryPublicationState::Integrated {
        bail!(
            "source {} needs integration review: {}",
            binding.transition,
            publication.summary
        );
    }
    Ok(())
}
fn identity(metadata: &std::fs::Metadata) -> (u64, u64, u64, i64, i64, u32) {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.ctime(),
        metadata.ctime_nsec(),
        metadata.mode(),
    )
}

async fn replace(artifacts: &PreparedArtifacts, rollback: bool) -> Result<()> {
    let source = artifacts
        .directory
        .join(if rollback { "baseline" } else { "candidate" });
    let expected = if rollback {
        &artifacts.baseline_digest
    } else {
        artifacts.signed_digest.as_ref().unwrap()
    }
    .clone();
    let previous = if rollback {
        artifacts.signed_digest.as_ref().unwrap()
    } else {
        &artifacts.baseline_digest
    }
    .clone();
    let destination = artifacts.destination.clone();
    let mode = artifacts.destination_mode;
    let prepared = artifacts
        .directory
        .join(format!("replace-{}", uuid::Uuid::new_v4()));
    let temporary = prepared.clone();
    let target = destination.clone();
    let state = tokio::task::spawn_blocking(move || -> Result<Option<_>> {
        let before = executable_metadata(&target)?;
        if before.mode() & 0o777 != mode {
            bail!("installed executable mode changed");
        }
        let current = artifact_digest(&target)?;
        let after = executable_metadata(&target)?;
        if identity(&before) != identity(&after) {
            bail!("installed executable changed while inspecting");
        }
        if current == expected {
            return Ok(None);
        }
        if current != previous {
            bail!("installed executable has later changes; replacement deferred");
        }
        copy_binary(&source, &temporary, &expected)?;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(mode))?;
        std::fs::File::open(&temporary)?.sync_all()?;
        Ok(Some(identity(&after)))
    })
    .await??;
    if let Some(expected_identity) = state {
        // No await between the last identity check and atomic rename. Cancelling
        // a copy can leave only a private temporary file, never a late replacement.
        if identity(&executable_metadata(&destination)?) != expected_identity {
            bail!("installed executable changed before atomic replacement");
        }
        std::fs::rename(&prepared, &destination)?;
        std::fs::File::open(destination.parent().context("destination parent missing")?)?
            .sync_all()?;
    }
    Ok(())
}
async fn verify_installed(
    artifacts: &PreparedArtifacts,
    rollback: bool,
    temporary: &Path,
    plan: &HarnessPlan,
    allowance: std::time::Duration,
) -> Result<()> {
    let expected = if rollback {
        &artifacts.baseline_digest
    } else {
        artifacts.signed_digest.as_ref().unwrap()
    }
    .clone();
    let mode = artifacts.destination_mode;
    let target = artifacts.destination.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        if executable_metadata(&target)?.mode() & 0o777 != mode
            || artifact_digest(&target)? != expected
        {
            bail!("installed executable postcondition failed");
        }
        Ok(())
    })
    .await??;
    signature(&artifacts.destination, false, temporary).await?;
    let actual = smoke_selected(
        &artifacts.destination,
        temporary,
        plan.vm.as_ref(),
        allowance,
    )
    .await?;
    let expected = if rollback {
        &artifacts.baseline_smoke
    } else {
        &artifacts.candidate_smoke
    };
    if &actual != expected {
        bail!("installed executable differs from verified version/help behavior");
    }
    let target = artifacts.destination.clone();
    let digest = if rollback {
        artifacts.baseline_digest.clone()
    } else {
        artifacts.signed_digest.clone().unwrap()
    };
    tokio::task::spawn_blocking(move || -> Result<()> {
        if executable_metadata(&target)?.mode() & 0o777 != mode
            || artifact_digest(&target)? != digest
        {
            bail!("installed executable changed during post-install checks");
        }
        Ok(())
    })
    .await??;
    Ok(())
}

#[cfg(test)]
pub async fn promote(vault: LearningVault, id: &str, repository: &Path) -> Result<Promotion> {
    let resources = crate::harness_automation::compatibility_resources(&vault, repository)?;
    promote_owned(vault, id, repository, resources.last()).await
}

pub async fn promote_owned(
    vault: LearningVault,
    id: &str,
    repository: &Path,
    repository_lease: Option<&ca_automation::runtime::ResourceLease>,
) -> Result<Promotion> {
    let result = Box::pin(run(vault.clone(), id, repository, false, repository_lease)).await;
    if let Err(error) = result {
        let cycle = load_harness_cycle(&vault, id)?.1;
        if cycle.stage != HarnessStage::Paused
            && report(&vault, id)?.is_some_and(|j| {
                matches!(
                    j.phase,
                    Phase::SourceAdopting | Phase::SourceAdopted | Phase::Replacing
                )
            })
        {
            let recovery = Box::pin(run(vault, id, repository, true, repository_lease)).await;
            return match recovery {
                Ok(_) => {
                    Err(error.context("activation failed; baseline binary and source restored"))
                }
                Err(recovery) => Err(error.context(format!(
                    "activation failed; rollback needs recovery: {recovery}"
                ))),
            };
        }
        return Err(error);
    }
    result
}
#[cfg(test)]
pub async fn rollback(vault: LearningVault, id: &str, repository: &Path) -> Result<Promotion> {
    let resources = crate::harness_automation::compatibility_resources(&vault, repository)?;
    rollback_owned(vault, id, repository, resources.last()).await
}

pub async fn rollback_owned(
    vault: LearningVault,
    id: &str,
    repository: &Path,
    repository_lease: Option<&ca_automation::runtime::ResourceLease>,
) -> Result<Promotion> {
    Box::pin(run(vault, id, repository, true, repository_lease)).await
}

async fn run(
    vault: LearningVault,
    id: &str,
    repository: &Path,
    rollback: bool,
    repository_lease: Option<&ca_automation::runtime::ResourceLease>,
) -> Result<Promotion> {
    let _worker = vault.acquire_worker_lock()?;
    let (plan, mut cycle) = load_harness_cycle(&vault, id)?;
    running(&vault, id)?;
    let repository = repository.canonicalize()?;
    let lease_root = publication_lease_root(&vault)?;
    let resource = format!("repository:{}", repository.display());
    if let Some(lease) = repository_lease {
        lease.validate_binding(&lease_root, &resource)?;
    } else {
        ca_automation::runtime::current_run_store()
            .context("source promotion requires repository ownership")?
            .require_adopted_resource(&resource)?;
    }
    let existing = report(&vault, id)?;
    if !rollback
        && existing
            .as_ref()
            .is_none_or(|journal| journal.phase == Phase::Prepared)
    {
        let binary = plan
            .activation_executable
            .as_deref()
            .context("source promotion requires a frozen executable")?;
        crate::harness_automation::require_review(&vault, id, &repository, binary).await?;
    }
    let mut journal = if let Some(journal) = existing {
        journal
    } else {
        if rollback || cycle.stage != HarnessStage::ReadyToPromote {
            bail!("no prepared promotion transaction");
        }
        let artifacts = crate::harness_activation::report(&vault, id)?
            .context("activation artifacts missing")?;
        let checkout: SourceCheckout = vault
            .read_mutable(&format!("harness/source-checkouts/{id}.json"))?
            .context("source checkout missing")?;
        if checkout.repository != repository {
            bail!("promotion repository changed");
        }
        let journal = Promotion {
            plan_digest: plan.digest.clone(),
            repository: repository.clone(),
            artifacts,
            candidate_source: checkout.candidate.context("candidate source missing")?,
            phase: Phase::Prepared,
            reverted_head: None,
            work: None,
        };
        validate(&journal, &plan)?;
        save(&vault, id, &journal)?;
        journal
    };
    if journal.repository != repository {
        bail!("promotion repository identity changed");
    }
    if !rollback
        && matches!(
            journal.phase,
            Phase::Restoring | Phase::BinaryRestored | Phase::Reverting | Phase::RolledBack
        )
    {
        bail!("rollback has started; use harness rollback to continue it");
    }
    // Only this journal's interrupted host work can be accounted at its full
    // reserved maximum. Unknown provider/other activity charges remain untouched.
    if let Some((work, maximum)) = &journal.work {
        if cycle.active_reservations.get(work) == Some(maximum) {
            if cycle.active_reservations.len() != 1 || !cycle.reservations.is_empty() {
                bail!("unrelated unsettled work prevents recovery");
            }
            update_harness_cycle(&vault, id, |state| {
                state.settle_active_time(&plan, work, *maximum)
            })?;
        } else if !cycle.completed_work.contains(work) {
            bail!("promotion activity receipt changed");
        }
        journal.work = None;
        save(&vault, id, &journal)?;
        cycle = load_harness_cycle(&vault, id)?.1;
    }
    if !cycle.reservations.is_empty() || !cycle.active_reservations.is_empty() {
        bail!("unsettled work prevents promotion or rollback");
    }
    if (!rollback && journal.phase == Phase::Active)
        || (rollback && journal.phase == Phase::RolledBack)
    {
        if !rollback && cycle.stage != HarnessStage::Promoted {
            crate::harness_guidance::publish_source(&vault, &plan, &journal)?;
        }
        update_harness_cycle(&vault, id, |state| {
            state.stage = if rollback {
                HarnessStage::RolledBack
            } else {
                HarnessStage::Promoted
            };
            state.resume_stage = None;
            Ok(())
        })?;
        return Ok(journal);
    }
    if !rollback {
        let decision = evaluate_harness(
            &plan,
            &journal.artifacts.candidate_digest,
            &cycle.trials,
            cycle.engineering_checks_passed,
        )?;
        if decision.verdict != HarnessVerdict::Promote
            || decision.trials_digest != journal.artifacts.trials_digest
            || cycle.candidate_commit.as_deref() != Some(&journal.candidate_source.head)
        {
            bail!("promotion evidence changed");
        }
        let prepared = crate::harness_activation::report(&vault, id)?
            .context("activation artifacts missing")?;
        if canonical_digest(&prepared)? != canonical_digest(&journal.artifacts)? {
            bail!("prepared artifacts changed since transaction admission");
        }
        crate::harness_guidance::publish_source(&vault, &plan, &journal)?;
    }
    let remaining = plan
        .settings
        .maximum_active_seconds
        .saturating_sub(cycle.active_seconds);
    // Retain one complete rollback allowance before any adoption attempt.
    if !rollback && remaining < 360 {
        bail!("promotion needs 180 seconds plus a retained rollback allowance");
    }
    let activity =
        HarnessActivity::begin_transition(vault.clone(), plan.clone(), remaining.min(180)).await?;
    journal.work = Some((activity.reservation().0.into(), activity.reservation().1));
    save(&vault, id, &journal)?;
    let result = activity
        .run(async {
            let temporary = tempfile::tempdir()?;
            if rollback {
                journal.phase = Phase::Restoring;
                save(&vault, id, &journal)?;
                running(&vault, id)?;
                replace(&journal.artifacts, true).await?;
                verify_installed(
                    &journal.artifacts,
                    true,
                    temporary.path(),
                    &plan,
                    activity.remaining()?,
                )
                .await?;
                journal.phase = Phase::BinaryRestored;
                save(&vault, id, &journal)?;
                running(&vault, id)?;
                let source_publications =
                    ca_automation::repository::RepositoryPublicationStore::new(
                        vault.resolve_relative("harness/source-publications")?,
                    );
                if source_publications
                    .load(&rollback_publication_id(id, &plan)?)?
                    .is_some()
                {
                    publish_source_rollback(
                        &vault,
                        id,
                        &plan,
                        &journal,
                        temporary.path(),
                        repository_lease,
                    )
                    .await?;
                }
                let current = source_snapshot(&repository).await?;
                if current != plan.baseline {
                    if at_candidate(&current, &plan, &journal.candidate_source) {
                        journal.phase = Phase::Reverting;
                        save(&vault, id, &journal)?;
                        publish_source_rollback(
                            &vault,
                            id,
                            &plan,
                            &journal,
                            temporary.path(),
                            repository_lease,
                        )
                        .await?;
                    } else {
                        let parent =
                            git(&repository, temporary.path(), &["rev-parse", "HEAD^"]).await?;
                        let message = git(
                            &repository,
                            temporary.path(),
                            &["show", "-s", "--format=%B", "HEAD"],
                        )
                        .await?;
                        if current.branch != plan.baseline.branch
                            || parent != journal.candidate_source.head
                            || current.tracked_tree_digest != plan.baseline.tracked_tree_digest
                            || !message.contains(&format!(
                                "This reverts commit {}.",
                                journal.candidate_source.head
                            ))
                        {
                            bail!("source has later changes; additive revert deferred");
                        }
                    }
                    let reverted = source_snapshot(&repository).await?;
                    if reverted.branch != plan.baseline.branch
                        || reverted.tracked_tree_digest != plan.baseline.tracked_tree_digest
                        || reverted.submodule_digest != plan.baseline.submodule_digest
                    {
                        bail!("source rollback postcondition failed");
                    }
                    journal.reverted_head = Some(reverted.head);
                }
                journal.phase = Phase::RolledBack;
            } else {
                let checked = journal.artifacts.clone();
                tokio::task::spawn_blocking(move || -> Result<()> {
                    let signed = checked.signed_digest.as_ref().unwrap();
                    for (name, expected) in [
                        ("baseline", &checked.baseline_digest),
                        ("candidate", signed),
                    ] {
                        let file = checked.directory.join(name);
                        executable_metadata(&file)?;
                        if artifact_digest(&file)? != *expected {
                            bail!("prepared executable changed before source adoption");
                        }
                    }
                    let metadata = executable_metadata(&checked.destination)?;
                    let current = artifact_digest(&checked.destination)?;
                    if metadata.mode() & 0o777 != checked.destination_mode
                        || (current != checked.baseline_digest && current != *signed)
                    {
                        bail!("installed executable changed before source adoption");
                    }
                    Ok(())
                })
                .await??;
                let checkout: SourceCheckout = vault
                    .read_mutable(&format!("harness/source-checkouts/{id}.json"))?
                    .context("checkout missing")?;
                let saved = crate::harness_source_generation::load(&vault, id)?
                    .context("source generation missing")?;
                let (_, harness, _) = crate::harness_source_build::load_candidate(&vault, id)?;
                if harness.digest()? != journal.artifacts.candidate_digest
                    || checkout.candidate.as_ref() != Some(&journal.candidate_source)
                    || checkout.repository != repository
                {
                    bail!("candidate source binding changed");
                }
                audit_candidate(&checkout, &saved, &plan, temporary.path()).await?;
                let current = source_snapshot(&repository).await?;
                if current == plan.baseline {
                    journal.phase = Phase::SourceAdopting;
                    save(&vault, id, &journal)?;
                    stage(&vault, id, HarnessStage::SourceAdopting)?;
                    running(&vault, id)?;
                    publish_source_candidate(
                        &vault,
                        id,
                        &plan,
                        &journal,
                        &checkout,
                        temporary.path(),
                        repository_lease,
                    )
                    .await?;
                } else if !at_candidate(&current, &plan, &journal.candidate_source) {
                    bail!("source changed before adoption; no source mutation attempted");
                }
                if !at_candidate(
                    &source_snapshot(&repository).await?,
                    &plan,
                    &journal.candidate_source,
                ) {
                    bail!("source adoption postcondition failed");
                }
                journal.phase = Phase::SourceAdopted;
                save(&vault, id, &journal)?;
                stage(&vault, id, HarnessStage::SourceAdopted)?;
                running(&vault, id)?;
                journal.phase = Phase::Replacing;
                save(&vault, id, &journal)?;
                replace(&journal.artifacts, false).await?;
                verify_installed(
                    &journal.artifacts,
                    false,
                    temporary.path(),
                    &plan,
                    activity.remaining()?,
                )
                .await?;
                if !at_candidate(
                    &source_snapshot(&repository).await?,
                    &plan,
                    &journal.candidate_source,
                ) {
                    bail!("source changed during executable activation");
                }
                journal.phase = Phase::Active;
            }
            save(&vault, id, &journal)?;
            Ok(())
        })
        .await;
    if !load_harness_cycle(&vault, id)?
        .1
        .active_reservations
        .contains_key(activity.reservation().0)
    {
        journal.work = None;
    }
    save(&vault, id, &journal)?;
    update_harness_cycle(&vault, id, |state| {
        if result.is_ok() {
            state.stage = if journal.phase == Phase::Active {
                HarnessStage::Promoted
            } else {
                HarnessStage::RolledBack
            };
            state.resume_stage = None;
            state.reason = if journal.phase == Phase::Active {
                "Verified source and signed executable activated"
            } else {
                "Baseline executable restored; source rollback verified"
            }
            .into();
        } else if !matches!(
            state.stage,
            HarnessStage::Paused | HarnessStage::BudgetExhausted
        ) {
            state.stage = HarnessStage::RecoveryRequired;
            state.reason = "Promotion transaction needs recovery; artifacts retained".into();
        }
        Ok(())
    })?;
    result?;
    Ok(journal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_team::self_improvement::{harness_trial_schedule, HarnessLane, HarnessTrial};

    struct Fixture {
        _root: tempfile::TempDir,
        vault: LearningVault,
        plan: HarnessPlan,
        repository: PathBuf,
        artifacts: PreparedArtifacts,
    }
    async fn fixture() -> Fixture {
        Box::pin(fixture_backend(None)).await
    }
    async fn fixture_backend(vm: Option<codex_team::self_improvement::FrozenHarnessVm>) -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let (vault, plan, repository) = Box::pin(
            crate::harness_source_build::tests::fixture_with_backend(root.path(), false, true, vm),
        )
        .await;
        let harness = crate::harness_source_build::build_and_admit(vault.clone(), &plan.id)
            .await
            .unwrap();
        update_harness_cycle(&vault, &plan.id, |state| {
            state.trials = harness_trial_schedule(&plan)?
                .into_iter()
                .map(|(case_id, repetition, lane)| HarnessTrial {
                    plan_digest: plan.digest.clone(),
                    candidate_digest: harness.digest().unwrap(),
                    route_digest: plan.route_digest.clone(),
                    case_id,
                    repetition,
                    lane,
                    verified_pass: true,
                    infrastructure_failure: false,
                    false_completion: false,
                    output_digest: "a".repeat(64),
                    cost_microusd: if lane == HarnessLane::Baseline {
                        100
                    } else {
                        80
                    },
                    elapsed_millis: 1,
                })
                .collect();
            state.decide(&plan)
        })
        .unwrap();
        crate::harness_source_checkout::import_candidate(vault.clone(), &plan.id, &repository)
            .await
            .unwrap();
        let artifacts = crate::harness_activation::prepare(vault.clone(), &plan.id)
            .await
            .unwrap();
        assert!(crate::harness_automation::require_review(
            &vault,
            &plan.id,
            &repository,
            &artifacts.destination
        )
        .await
        .is_err());
        assert!(report(&vault, &plan.id).unwrap().is_none());
        let review = crate::harness_automation::preview_review(
            &vault,
            &plan.id,
            &repository,
            &artifacts.destination,
        )
        .await
        .unwrap();
        assert!(crate::harness_automation::confirm_review(
            &vault,
            &plan.id,
            &repository,
            &artifacts.destination,
            &"0".repeat(64)
        )
        .await
        .is_err());
        crate::harness_automation::confirm_review(
            &vault,
            &plan.id,
            &repository,
            &artifacts.destination,
            &review.digest,
        )
        .await
        .unwrap();
        Fixture {
            _root: root,
            vault,
            plan,
            repository,
            artifacts,
        }
    }

    #[tokio::test]
    async fn aut259_source_promotion_refuses_changed_evaluation_after_review() {
        // Synthetic settled evidence exercises the review guard before any
        // build, signing or publication. The ignored process fixtures below
        // exercise those independent host gates with real candidate binaries.
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let repository = root.join("repository");
        std::fs::create_dir_all(repository.join("src")).unwrap();
        std::fs::write(repository.join("src/repair.rs"), b"baseline source").unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@invalid",
                "commit",
                "-qm",
                "baseline",
            ],
        ] {
            run_git(
                &repository,
                &root,
                args.into_iter().map(Into::into).collect(),
            )
            .await
            .unwrap();
        }
        let binary = root.join("baseline-binary");
        std::fs::write(&binary, b"baseline executable bytes").unwrap();
        let vault = LearningVault::open_at(root.join("vault")).unwrap();
        let (mut plan, _, _) = crate::harness_generation::tests::inputs();
        plan.baseline = source_snapshot(&repository).await.unwrap();
        plan.baseline_binary_digest = artifact_digest(&binary).unwrap();
        plan.activation_executable = Some(binary.clone());
        plan.source_files.insert(
            "src/repair.rs".into(),
            artifact_digest(&repository.join("src/repair.rs")).unwrap(),
        );
        plan.seal().unwrap();
        codex_team::self_improvement::start_harness_cycle(&vault, &plan, 0).unwrap();
        update_harness_cycle(&vault, &plan.id, |state| {
            state.candidate_digest = Some("b".repeat(64));
            state.engineering_checks_passed = true;
            state.stage = HarnessStage::CandidateReady;
            state.trials = harness_trial_schedule(&plan)?
                .into_iter()
                .map(|(case_id, repetition, lane)| HarnessTrial {
                    plan_digest: plan.digest.clone(),
                    candidate_digest: "b".repeat(64),
                    route_digest: plan.route_digest.clone(),
                    case_id,
                    repetition,
                    lane,
                    verified_pass: true,
                    infrastructure_failure: false,
                    false_completion: false,
                    output_digest: "a".repeat(64),
                    cost_microusd: if lane == HarnessLane::Baseline {
                        100
                    } else {
                        80
                    },
                    elapsed_millis: 1,
                })
                .collect();
            state.decide(&plan)
        })
        .unwrap();
        let review =
            crate::harness_automation::preview_review(&vault, &plan.id, &repository, &binary)
                .await
                .unwrap();
        crate::harness_automation::confirm_review(
            &vault,
            &plan.id,
            &repository,
            &binary,
            &review.digest,
        )
        .await
        .unwrap();
        let baseline = source_snapshot(&repository).await.unwrap();
        update_harness_cycle(&vault, &plan.id, |state| {
            state.trials[0].output_digest = "b".repeat(64);
            Ok(())
        })
        .unwrap();
        assert!(promote(vault.clone(), &plan.id, &repository).await.is_err());
        assert!(report(&vault, &plan.id).unwrap().is_none());
        assert_eq!(source_snapshot(&repository).await.unwrap(), baseline);
        assert_eq!(
            artifact_digest(&binary).unwrap(),
            plan.baseline_binary_digest
        );
    }

    #[tokio::test]
    async fn aut259_source_publication_preserves_reviewed_binding_and_exact_checkout() {
        // The shared publisher receives settled harness authority. This fixture
        // exercises its Git boundary without executing or installing a binary.
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let repository = root.join("repository");
        std::fs::create_dir(&repository).unwrap();
        std::fs::write(repository.join("source.rs"), "baseline\n").unwrap();
        for args in [
            vec!["init", "-qb", "fixture"],
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@invalid",
                "commit",
                "-qm",
                "baseline",
            ],
            vec!["branch", "other"],
        ] {
            run_git(
                &repository,
                &root,
                args.into_iter().map(Into::into).collect(),
            )
            .await
            .unwrap();
        }
        let vault = LearningVault::open_at(root.join("vault")).unwrap();
        let candidate = root.join("candidate");
        run_git(
            &repository,
            &root,
            vec![
                "clone".into(),
                "--local".into(),
                "--no-hardlinks".into(),
                "--quiet".into(),
                repository.as_os_str().into(),
                candidate.as_os_str().into(),
            ],
        )
        .await
        .unwrap();
        std::fs::write(candidate.join("source.rs"), "candidate\n").unwrap();
        for args in [
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@invalid",
                "commit",
                "-qm",
                "candidate",
            ],
        ] {
            run_git(
                &candidate,
                &root,
                args.into_iter().map(Into::into).collect(),
            )
            .await
            .unwrap();
        }
        let (mut plan, _, _) = crate::harness_generation::tests::inputs();
        plan.baseline = source_snapshot(&repository).await.unwrap();
        plan.seal().unwrap();
        let candidate_source = source_snapshot(&candidate).await.unwrap();
        run_git(
            &repository,
            &root,
            vec![
                "fetch".into(),
                "--no-tags".into(),
                "--no-write-fetch-head".into(),
                candidate.as_os_str().into(),
                candidate_source.head.clone().into(),
            ],
        )
        .await
        .unwrap();
        let mut checkout = SourceCheckout {
            plan_digest: plan.digest.clone(),
            generation_digest: "a".repeat(64),
            repository: repository.clone(),
            workspace: candidate.clone(),
            started: false,
            candidate: Some(candidate_source.clone()),
        };
        let mut journal = Promotion {
            plan_digest: plan.digest.clone(),
            repository: repository.clone(),
            artifacts: PreparedArtifacts {
                guidance: None,
                plan_digest: plan.digest.clone(),
                candidate_digest: "b".repeat(64),
                trials_digest: "c".repeat(64),
                destination: root.join("not-installed"),
                directory: root.join("not-built"),
                baseline_digest: "d".repeat(64),
                evaluated_digest: "e".repeat(64),
                destination_mode: 0o755,
                signed_digest: Some("f".repeat(64)),
                baseline_smoke: vec!["a".repeat(64); 2],
                candidate_smoke: vec!["b".repeat(64); 2],
                ready: true,
            },
            candidate_source: candidate_source.clone(),
            phase: Phase::SourceAdopting,
            reverted_head: None,
            work: None,
        };
        let resources =
            crate::harness_automation::compatibility_resources(&vault, &repository).unwrap();
        let resource = resources.last();
        run_git(
            &repository,
            &root,
            vec!["switch".into(), "-q".into(), "other".into()],
        )
        .await
        .unwrap();
        assert!(publish_source_candidate(
            &vault, &plan.id, &plan, &journal, &checkout, &root, resource
        )
        .await
        .is_err());
        assert_eq!(
            git(&repository, &root, &["rev-parse", "refs/heads/fixture"])
                .await
                .unwrap(),
            plan.baseline.head
        );
        assert_eq!(
            std::fs::read(repository.join("source.rs")).unwrap(),
            b"baseline\n"
        );
        assert!(candidate.join("source.rs").is_file());
        run_git(
            &repository,
            &root,
            vec!["switch".into(), "-q".into(), "fixture".into()],
        )
        .await
        .unwrap();
        assert!(
            publish_source_candidate(&vault, &plan.id, &plan, &journal, &checkout, &root, resource)
                .await
                .is_err(),
            "a retained stale publication must not replay when HEAD returns"
        );
        // A separate exact authorization has its own immutable publication
        // identity; the old retained journal stays available for inspection.
        plan.id = "publication-reviewed".into();
        plan.seal().unwrap();
        journal.plan_digest = plan.digest.clone();
        journal.artifacts.plan_digest = plan.digest.clone();
        checkout.plan_digest = plan.digest.clone();
        publish_source_candidate(
            &vault, &plan.id, &plan, &journal, &checkout, &root, resource,
        )
        .await
        .unwrap();
        assert!(at_candidate(
            &source_snapshot(&repository).await.unwrap(),
            &plan,
            &candidate_source
        ));
        publish_source_rollback(&vault, &plan.id, &plan, &journal, &root, resource)
            .await
            .unwrap();
        let reverted = source_snapshot(&repository).await.unwrap();
        assert_eq!(reverted.branch, plan.baseline.branch);
        assert_eq!(
            reverted.tracked_tree_digest,
            plan.baseline.tracked_tree_digest
        );
        assert_eq!(
            git(&repository, &root, &["rev-parse", "HEAD^"])
                .await
                .unwrap(),
            candidate_source.head
        );
        assert!(
            git(&repository, &root, &["show", "-s", "--format=%B", "HEAD"])
                .await
                .unwrap()
                .contains(&format!("This reverts commit {}.", candidate_source.head))
        );
        publish_source_rollback(&vault, &plan.id, &plan, &journal, &root, resource)
            .await
            .unwrap();
        assert_eq!(source_snapshot(&repository).await.unwrap(), reverted);
        journal.artifacts.trials_digest = "0".repeat(64);
        assert!(publish_source_candidate(
            &vault, &plan.id, &plan, &journal, &checkout, &root, resource
        )
        .await
        .is_err());
        assert_eq!(
            std::fs::read(repository.join("source.rs")).unwrap(),
            b"baseline\n"
        );
        assert!(!journal.artifacts.destination.exists());
    }

    #[tokio::test]
    #[ignore = "requires dedicated VM; builds a tiny source candidate and probes activation/rollback in fresh guests"]
    async fn hrs253_vm_source_activation_and_rollback_use_guest_probes() {
        let binding = std::env::var_os("CA_HARNESS_VM_TEST_BINDING").expect("VM binding");
        let mut frozen: codex_team::self_improvement::FrozenHarnessVm =
            serde_json::from_slice(&std::fs::read(binding).unwrap()).unwrap();
        frozen.validate().unwrap();
        frozen.adapter_digest = env!("CA_HARNESS_ADAPTER_DIGEST").into();
        let f = Box::pin(fixture_backend(Some(frozen))).await;
        let active = promote(f.vault.clone(), &f.plan.id, &f.repository)
            .await
            .unwrap();
        assert_eq!(active.phase, Phase::Active);
        assert_eq!(
            artifact_digest(&f.artifacts.destination).unwrap(),
            f.artifacts.signed_digest.as_deref().unwrap()
        );
        assert_eq!(
            source_snapshot(&f.repository).await.unwrap().head,
            active.candidate_source.head
        );
        assert!(active.artifacts.guidance.is_some());
        let recovered = rollback(f.vault.clone(), &f.plan.id, &f.repository)
            .await
            .unwrap();
        assert_eq!(recovered.phase, Phase::RolledBack);
        assert_eq!(
            artifact_digest(&f.artifacts.destination).unwrap(),
            f.plan.baseline_binary_digest
        );
        assert_eq!(
            source_snapshot(&f.repository)
                .await
                .unwrap()
                .tracked_tree_digest,
            f.plan.baseline.tracked_tree_digest
        );
        let settled = load_harness_cycle(&f.vault, &f.plan.id).unwrap().1;
        assert!(settled.active_reservations.is_empty() && settled.reservations.is_empty());
        assert!(settled.active_seconds > 0);
        eprintln!("VM_PROMOTION_ROLLBACK_SECONDS={}", settled.active_seconds);
    }

    #[tokio::test]
    #[ignore = "runs real source adoption, signed replacement and rollback in a disposable macOS project"]
    async fn hrs253_promotion_activates_and_rollback_preserves_later_work() {
        let f = Box::pin(fixture()).await;
        let active = promote(f.vault.clone(), &f.plan.id, &f.repository)
            .await
            .unwrap();
        assert_eq!(active.phase, Phase::Active);
        let carried = active.artifacts.guidance.as_ref().unwrap();
        assert_eq!(
            crate::harness_guidance::baseline_hints(
                &f.vault,
                &carried.source,
                &carried.model,
                &carried.options,
            )
            .unwrap(),
            [crate::harness_hints::ExecutionHint::DependencyProbes]
                .into_iter()
                .collect(),
        );
        assert_eq!(
            crate::harness_guidance::baseline_instructions(
                &f.vault,
                &carried.source,
                &carried.model,
                &carried.options,
            )
            .unwrap(),
            carried.instructions,
        );
        assert_eq!(
            load_harness_cycle(&f.vault, &f.plan.id).unwrap().1.stage,
            HarnessStage::Promoted
        );
        assert_eq!(
            artifact_digest(&f.artifacts.destination).unwrap(),
            f.artifacts.signed_digest.as_deref().unwrap()
        );
        assert_eq!(
            source_snapshot(&f.repository).await.unwrap().head,
            active.candidate_source.head
        );
        std::fs::write(f.repository.join("user-work.txt"), "keep this").unwrap();
        assert!(rollback(f.vault.clone(), &f.plan.id, &f.repository)
            .await
            .is_err());
        assert_eq!(
            std::fs::read_to_string(f.repository.join("user-work.txt")).unwrap(),
            "keep this"
        );
        assert_eq!(
            artifact_digest(&f.artifacts.destination).unwrap(),
            f.plan.baseline_binary_digest
        );
        assert_eq!(
            report(&f.vault, &f.plan.id).unwrap().unwrap().phase,
            Phase::BinaryRestored
        );
        std::fs::remove_file(f.repository.join("user-work.txt")).unwrap();
        let reverted = rollback(f.vault.clone(), &f.plan.id, &f.repository)
            .await
            .unwrap();
        assert_eq!(reverted.phase, Phase::RolledBack);
        assert!(
            crate::harness_guidance::baseline_instructions(
                &f.vault,
                &carried.source,
                &carried.model,
                &carried.options,
            )
            .is_err(),
            "a rolled-back source build cannot consume carried guidance"
        );
        let after = source_snapshot(&f.repository).await.unwrap();
        assert_eq!(
            after.tracked_tree_digest,
            f.plan.baseline.tracked_tree_digest
        );
        assert_ne!(after.head, f.plan.baseline.head);
        assert_eq!(
            git(&f.repository, f._root.path(), &["rev-parse", "HEAD^"])
                .await
                .unwrap(),
            active.candidate_source.head
        );
        assert_eq!(
            load_harness_cycle(&f.vault, &f.plan.id).unwrap().1.stage,
            HarnessStage::RolledBack
        );
        let recovered = rollback(f.vault.clone(), &f.plan.id, &f.repository)
            .await
            .unwrap();
        assert_eq!(recovered.reverted_head, reverted.reverted_head);
        assert_eq!(
            artifact_digest(&f.artifacts.destination).unwrap(),
            f.plan.baseline_binary_digest
        );
    }

    #[tokio::test]
    #[ignore = "runs real crash recovery and signed activation in a disposable macOS project"]
    async fn hrs253_promotion_recovers_adopted_source_without_resetting_time() {
        for already_installed in [false, true] {
            let f = Box::pin(fixture()).await;
            let checkout: SourceCheckout = f
                .vault
                .read_mutable(&format!("harness/source-checkouts/{}.json", f.plan.id))
                .unwrap()
                .unwrap();
            let candidate = checkout.candidate.unwrap();
            let before = load_harness_cycle(&f.vault, &f.plan.id)
                .unwrap()
                .1
                .active_seconds;
            update_harness_cycle(&f.vault, &f.plan.id, |state| {
                state.stage = if already_installed {
                    HarnessStage::SourceAdopted
                } else {
                    HarnessStage::SourceAdopting
                };
                state.reserve_transition_time(&f.plan, "interrupted-transition", 180)
            })
            .unwrap();
            let journal = Promotion {
                plan_digest: f.plan.digest.clone(),
                repository: f.repository.canonicalize().unwrap(),
                artifacts: f.artifacts.clone(),
                candidate_source: candidate.clone(),
                phase: if already_installed {
                    Phase::Replacing
                } else {
                    Phase::SourceAdopting
                },
                reverted_head: None,
                work: Some(("interrupted-transition".into(), 180)),
            };
            save(&f.vault, &f.plan.id, &journal).unwrap();
            git(
                &f.repository,
                f._root.path(),
                &["merge", "--ff-only", "--no-edit", &candidate.head],
            )
            .await
            .unwrap();
            if already_installed {
                replace(&f.artifacts, false).await.unwrap();
            }
            let before_inode = std::fs::metadata(&f.artifacts.destination).unwrap().ino();
            let active = promote(f.vault.clone(), &f.plan.id, &f.repository)
                .await
                .unwrap();
            assert_eq!(active.phase, Phase::Active);
            if already_installed {
                assert_eq!(
                    std::fs::metadata(&f.artifacts.destination).unwrap().ino(),
                    before_inode
                );
            }
            let cycle = load_harness_cycle(&f.vault, &f.plan.id).unwrap().1;
            assert!(cycle.active_seconds >= before + 180);
            assert!(cycle.active_reservations.is_empty());
            assert!(cycle.completed_work.contains("interrupted-transition"));
            assert_eq!(
                git(
                    &f.repository,
                    f._root.path(),
                    &["rev-list", "--count", "HEAD"]
                )
                .await
                .unwrap(),
                "2"
            );
            rollback(f.vault.clone(), &f.plan.id, &f.repository)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    #[ignore = "runs a real frozen candidate build in a disposable macOS project"]
    async fn hrs253_promotion_refuses_unknown_charges_and_changed_executable() {
        let f = Box::pin(fixture()).await;
        update_harness_cycle(&f.vault, &f.plan.id, |state| {
            state.reservations.insert("unknown-post".into(), 123);
            Ok(())
        })
        .unwrap();
        assert!(promote(f.vault.clone(), &f.plan.id, &f.repository)
            .await
            .is_err());
        assert_eq!(
            load_harness_cycle(&f.vault, &f.plan.id)
                .unwrap()
                .1
                .reservations["unknown-post"],
            123
        );
        assert_eq!(
            source_snapshot(&f.repository).await.unwrap(),
            f.plan.baseline
        );
        update_harness_cycle(&f.vault, &f.plan.id, |state| {
            state.reservations.clear();
            Ok(())
        })
        .unwrap();
        std::fs::write(&f.artifacts.destination, "later user executable").unwrap();
        assert!(promote(f.vault.clone(), &f.plan.id, &f.repository)
            .await
            .is_err());
        assert_eq!(
            std::fs::read_to_string(&f.artifacts.destination).unwrap(),
            "later user executable"
        );
        assert_eq!(
            source_snapshot(&f.repository).await.unwrap(),
            f.plan.baseline
        );
    }

    #[tokio::test]
    #[ignore = "runs real post-install failure recovery in a disposable macOS project"]
    async fn hrs253_promotion_failed_postcheck_restores_binary_and_additive_source_history() {
        let f = Box::pin(fixture()).await;
        let mut expected = f.artifacts.clone();
        // A mismatched expected smoke receipt deterministically fails only after
        // the candidate has been installed, exercising the recovery transaction.
        expected.candidate_smoke = vec!["0".repeat(64); 2];
        f.vault
            .write_mutable(
                &format!("harness/source/{}-activation.json", f.plan.id),
                &expected,
            )
            .unwrap();
        let error = promote(f.vault.clone(), &f.plan.id, &f.repository)
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("baseline binary and source restored"));
        let journal = report(&f.vault, &f.plan.id).unwrap().unwrap();
        assert_eq!(journal.phase, Phase::RolledBack);
        assert_eq!(
            load_harness_cycle(&f.vault, &f.plan.id).unwrap().1.stage,
            HarnessStage::RolledBack
        );
        assert_eq!(
            artifact_digest(&f.artifacts.destination).unwrap(),
            f.plan.baseline_binary_digest
        );
        assert_eq!(
            source_snapshot(&f.repository)
                .await
                .unwrap()
                .tracked_tree_digest,
            f.plan.baseline.tracked_tree_digest
        );
        assert_eq!(
            git(
                &f.repository,
                f._root.path(),
                &["rev-list", "--count", "HEAD"]
            )
            .await
            .unwrap(),
            "3"
        );
    }
}
