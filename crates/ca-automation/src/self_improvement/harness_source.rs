//! Journaled source adoption. Binary activation is a separate subsequent step.
use super::{
    evaluate_harness, fast_forward_source_candidate, inspect_source_candidate, load_harness_cycle,
    observe_clean_repository, update_harness_cycle, HarnessStage, HarnessVerdict, LearningVault,
    SourceRepositorySnapshotV1,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub use super::harness_protected::HARNESS_PROTECTED_SOURCE;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessSourcePhase {
    Prepared,
    Adopted,
    Reverting,
    Reverted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessSourceJournal {
    pub schema: String,
    pub cycle_id: String,
    pub repository: std::path::PathBuf,
    pub plan_digest: String,
    pub trials_digest: String,
    pub baseline: SourceRepositorySnapshotV1,
    pub candidate_commit: String,
    pub inspection_digest: String,
    pub phase: HarnessSourcePhase,
    pub reverted_head: Option<String>,
}

fn journal_path(id: &str) -> String {
    format!("harness/source/{id}.json")
}

/// Only the host calls this after all matched trial receipts and engineering
/// gates are complete. Restarting never re-evaluates against a new baseline.
pub fn adopt_harness_source(
    vault: &LearningVault,
    id: &str,
    repository: &Path,
) -> Result<HarnessSourceJournal> {
    let _worker = vault.acquire_worker_lock()?;
    let (plan, cycle) = load_harness_cycle(vault, id)?;
    let repository_identity = repository.canonicalize()?;
    if !matches!(
        cycle.stage,
        HarnessStage::ReadyToPromote | HarnessStage::SourceAdopting | HarnessStage::SourceAdopted
    ) {
        bail!("source adoption requires a completed promotion decision");
    }
    if !cycle.reservations.is_empty() || !cycle.active_reservations.is_empty() {
        bail!("unsettled provider charges prevent source adoption");
    }
    let decision = evaluate_harness(
        &plan,
        cycle
            .candidate_digest
            .as_deref()
            .context("candidate identity missing")?,
        &cycle.trials,
        cycle.engineering_checks_passed,
    )?;
    if decision.verdict != HarnessVerdict::Promote {
        bail!("current frozen receipts do not authorize promotion");
    }
    let candidate = cycle
        .candidate_commit
        .as_deref()
        .context("candidate commit missing")?;
    let protected: Vec<_> = HARNESS_PROTECTED_SOURCE
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let mut journal =
        if let Some(journal) = vault.read_mutable::<HarnessSourceJournal>(&journal_path(id))? {
            if journal.repository != repository_identity
                || journal.schema != "ca.harness_source.v1"
                || journal.cycle_id != id
                || journal.plan_digest != plan.digest
                || journal.trials_digest != decision.trials_digest
                || journal.candidate_commit != candidate
                || journal.baseline != plan.baseline
            {
                bail!("source journal does not match frozen promotion evidence");
            }
            journal
        } else {
            let inspection =
                inspect_source_candidate(repository, &plan.baseline, candidate, &protected)?;
            let journal = HarnessSourceJournal {
                schema: "ca.harness_source.v1".into(),
                cycle_id: id.into(),
                repository: repository_identity,
                plan_digest: plan.digest.clone(),
                trials_digest: decision.trials_digest,
                baseline: plan.baseline.clone(),
                candidate_commit: inspection.candidate_commit,
                inspection_digest: inspection.digest,
                phase: HarnessSourcePhase::Prepared,
                reverted_head: None,
            };
            vault.write_mutable(&journal_path(id), &journal)?;
            journal
        };
    if !matches!(
        journal.phase,
        HarnessSourcePhase::Prepared | HarnessSourcePhase::Adopted
    ) {
        bail!("source transaction is reverting or already reverted");
    }
    update_harness_cycle(vault, id, |c| {
        if !matches!(
            c.stage,
            HarnessStage::ReadyToPromote
                | HarnessStage::SourceAdopting
                | HarnessStage::SourceAdopted
        ) {
            bail!("cycle paused or changed before source transition");
        }
        c.stage = HarnessStage::SourceAdopting;
        Ok(())
    })?;
    let current = observe_clean_repository(repository)?;
    if current == plan.baseline {
        if journal.phase != HarnessSourcePhase::Prepared {
            bail!("adopted source unexpectedly returned to baseline");
        }
        let receipt =
            fast_forward_source_candidate(repository, &plan.baseline, candidate, &protected)?;
        if receipt.candidate_inspection_digest != journal.inspection_digest {
            bail!("candidate inspection drifted");
        }
    } else if current.branch != plan.baseline.branch || current.head != journal.candidate_commit {
        bail!("source drifted during adoption; no recovery mutation was made");
    }
    journal.phase = HarnessSourcePhase::Adopted;
    vault.write_mutable(&journal_path(id), &journal)?;
    update_harness_cycle(vault, id, |c| {
        if c.stage == HarnessStage::Paused {
            c.resume_stage = Some(HarnessStage::SourceAdopted);
        } else {
            c.stage = HarnessStage::SourceAdopted;
        }
        Ok(())
    })?;
    Ok(journal)
}

/// Ordinary additive revert, only while the adopted branch remains clean and
/// unchanged. A later user commit or any uncommitted edit causes deferral.
pub fn revert_harness_source(
    vault: &LearningVault,
    id: &str,
    repository: &Path,
) -> Result<HarnessSourceJournal> {
    let _worker = vault.acquire_worker_lock()?;
    let (plan, cycle) = load_harness_cycle(vault, id)?;
    let repository_identity = repository.canonicalize()?;
    let mut journal: HarnessSourceJournal = vault
        .read_mutable(&journal_path(id))?
        .context("source adoption journal missing")?;
    if journal.repository != repository_identity
        || journal.plan_digest != plan.digest
        || journal.cycle_id != id
        || journal.baseline != plan.baseline
        || cycle.candidate_commit.as_deref() != Some(&journal.candidate_commit)
    {
        bail!("rollback journal binding mismatch");
    }
    let current = observe_clean_repository(repository)?;
    if journal.phase == HarnessSourcePhase::Reverted {
        if current.branch != plan.baseline.branch
            || journal.reverted_head.as_deref() != Some(&current.head)
        {
            bail!("source changed after rollback");
        }
        return Ok(journal);
    }
    if !matches!(
        journal.phase,
        HarnessSourcePhase::Adopted | HarnessSourcePhase::Reverting
    ) {
        bail!("source has not been adopted");
    }
    if current.branch != plan.baseline.branch {
        bail!("source branch changed; rollback deferred");
    }
    if current.head != journal.candidate_commit {
        // A crash after Git committed but before the receipt: accept only the
        // exact parent and baseline tree; never reset or infer from timestamps.
        if journal.phase != HarnessSourcePhase::Reverting
            || git_text(repository, &["rev-parse", "HEAD^"])? != journal.candidate_commit
            || git_text(repository, &["rev-parse", "HEAD^{tree}"])?
                != git_text(
                    repository,
                    &["rev-parse", &format!("{}^{{tree}}", plan.baseline.head)],
                )?
        {
            bail!("source has later changes; rollback deferred");
        }
    } else {
        journal.phase = HarnessSourcePhase::Reverting;
        vault.write_mutable(&journal_path(id), &journal)?;
        git_text(
            repository,
            &[
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
                "revert",
                "--no-edit",
                &journal.candidate_commit,
            ],
        )?;
    }
    let after = observe_clean_repository(repository)?;
    journal.phase = HarnessSourcePhase::Reverted;
    journal.reverted_head = Some(after.head);
    vault.write_mutable(&journal_path(id), &journal)?;
    // Binary rollback is separate. Do not claim full rollback from a source receipt.
    update_harness_cycle(vault, id, |c| {
        c.stage = HarnessStage::RecoveryRequired;
        c.reason = "Source reverted; binary recovery must be verified separately".into();
        Ok(())
    })?;
    Ok(journal)
}

fn git_text(repository: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repository)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        bail!("Git source transition failed; inspect the recovery journal and repository");
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::self_improvement::{
        start_harness_cycle, HarnessCase, HarnessLane, HarnessPlan, HarnessSettings, HarnessTrial,
        HARNESS_SCHEMA,
    };
    struct Fixture {
        _root: tempfile::TempDir,
        repo: std::path::PathBuf,
        vault: LearningVault,
        plan: HarnessPlan,
        candidate: String,
    }
    fn fixture(path: &str) -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        git_text(&repo, &["init", "-b", "main"]).unwrap();
        git_text(&repo, &["config", "user.name", "Harness Test"]).unwrap();
        git_text(&repo, &["config", "user.email", "test@example.invalid"]).unwrap();
        git_text(&repo, &["config", "commit.gpgsign", "false"]).unwrap();
        let file = repo.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "baseline\n").unwrap();
        git_text(&repo, &["add", "."]).unwrap();
        git_text(&repo, &["commit", "-m", "baseline"]).unwrap();
        let baseline = observe_clean_repository(&repo).unwrap();
        git_text(&repo, &["checkout", "-b", "candidate"]).unwrap();
        std::fs::write(file, "candidate\n").unwrap();
        git_text(&repo, &["commit", "-am", "candidate"]).unwrap();
        let candidate = git_text(&repo, &["rev-parse", "HEAD"]).unwrap();
        git_text(&repo, &["checkout", "main"]).unwrap();
        let vault = LearningVault::open_at(root.path().join("vault")).unwrap();
        let mut plan = HarnessPlan {
            vm: None,
            preparation_seconds: None,
            guidance_source: None,
            activation_executable: None,
            source_files: Default::default(),
            schema: HARNESS_SCHEMA.into(),
            id: "source-test".into(),
            baseline,
            baseline_binary_digest: "a".repeat(64),
            route_digest: "b".repeat(64),
            price_digest: "c".repeat(64),
            evidence_ids: vec!["event".into()],
            cases: (0..3)
                .map(|i| HarnessCase {
                    id: format!("case-{i}"),
                    input_digest: "d".repeat(64),
                    verifier_digest: "e".repeat(64),
                    grants_digest: "f".repeat(64),
                    held_out: i > 0,
                    predicted_fix: i == 0,
                })
                .collect(),
            repetitions: 3,
            settings: HarnessSettings::default(),
            digest: String::new(),
        };
        plan.seal().unwrap();
        start_harness_cycle(&vault, &plan, 0).unwrap();
        update_harness_cycle(&vault, &plan.id, |cycle| {
            cycle.candidate_digest = Some("f".repeat(64));
            cycle.candidate_commit = Some(candidate.clone());
            cycle.engineering_checks_passed = true;
            cycle.stage = HarnessStage::Evaluating;
            for case in &plan.cases {
                for repetition in 0..3 {
                    for lane in [HarnessLane::Baseline, HarnessLane::Candidate] {
                        cycle.trials.push(HarnessTrial {
                            plan_digest: plan.digest.clone(),
                            candidate_digest: "f".repeat(64),
                            route_digest: plan.route_digest.clone(),
                            case_id: case.id.clone(),
                            repetition,
                            lane,
                            verified_pass: case.held_out || lane == HarnessLane::Candidate,
                            infrastructure_failure: false,
                            false_completion: false,
                            output_digest: "a".repeat(64),
                            cost_microusd: 0,
                            elapsed_millis: 1,
                        });
                    }
                }
            }
            cycle.decide(&plan)
        })
        .unwrap();
        Fixture {
            _root: root,
            repo,
            vault,
            plan,
            candidate,
        }
    }
    #[test]
    fn source_adoption_and_additive_revert_preserve_history() {
        let f = fixture("src/example.rs");
        let receipt = adopt_harness_source(&f.vault, &f.plan.id, &f.repo).unwrap();
        assert_eq!(receipt.phase, HarnessSourcePhase::Adopted);
        assert_eq!(observe_clean_repository(&f.repo).unwrap().head, f.candidate);
        assert_eq!(
            load_harness_cycle(&f.vault, &f.plan.id).unwrap().1.stage,
            HarnessStage::SourceAdopted
        );
        let reverted = revert_harness_source(&f.vault, &f.plan.id, &f.repo).unwrap();
        assert_eq!(reverted.phase, HarnessSourcePhase::Reverted);
        assert_eq!(
            std::fs::read_to_string(f.repo.join("src/example.rs")).unwrap(),
            "baseline\n"
        );
        assert_eq!(
            git_text(&f.repo, &["rev-parse", "HEAD^"]).unwrap(),
            f.candidate
        );
        assert_eq!(
            revert_harness_source(&f.vault, &f.plan.id, &f.repo)
                .unwrap()
                .reverted_head,
            reverted.reverted_head
        );
    }
    #[test]
    fn source_adoption_recovers_after_git_before_receipt() {
        let f = fixture("src/example.rs");
        adopt_harness_source(&f.vault, &f.plan.id, &f.repo).unwrap();
        let mut journal: HarnessSourceJournal = f
            .vault
            .read_mutable(&journal_path(&f.plan.id))
            .unwrap()
            .unwrap();
        journal.phase = HarnessSourcePhase::Prepared;
        f.vault
            .write_mutable(&journal_path(&f.plan.id), &journal)
            .unwrap();
        update_harness_cycle(&f.vault, &f.plan.id, |c| {
            c.stage = HarnessStage::SourceAdopting;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            adopt_harness_source(&f.vault, &f.plan.id, &f.repo)
                .unwrap()
                .phase,
            HarnessSourcePhase::Adopted
        );
        assert_eq!(
            git_text(&f.repo, &["rev-list", "--count", "HEAD"]).unwrap(),
            "2"
        );
    }
    #[test]
    fn dirty_source_and_protected_host_edits_never_adopt() {
        let f = fixture("src/example.rs");
        std::fs::write(f.repo.join("local.txt"), "user work").unwrap();
        assert!(adopt_harness_source(&f.vault, &f.plan.id, &f.repo).is_err());
        assert_eq!(
            git_text(&f.repo, &["rev-parse", "HEAD"]).unwrap(),
            f.plan.baseline.head
        );
        assert_eq!(
            std::fs::read_to_string(f.repo.join("local.txt")).unwrap(),
            "user work"
        );
        let protected = fixture("crates/codex-team/src/self_improvement/harness.rs");
        assert!(
            adopt_harness_source(&protected.vault, &protected.plan.id, &protected.repo).is_err()
        );
    }
    #[test]
    fn later_user_commit_defers_revert() {
        let f = fixture("src/example.rs");
        adopt_harness_source(&f.vault, &f.plan.id, &f.repo).unwrap();
        std::fs::write(f.repo.join("user.txt"), "later work").unwrap();
        git_text(&f.repo, &["add", "."]).unwrap();
        git_text(&f.repo, &["commit", "-m", "user work"]).unwrap();
        let head = git_text(&f.repo, &["rev-parse", "HEAD"]).unwrap();
        assert!(revert_harness_source(&f.vault, &f.plan.id, &f.repo).is_err());
        assert_eq!(git_text(&f.repo, &["rev-parse", "HEAD"]).unwrap(), head);
    }
}
