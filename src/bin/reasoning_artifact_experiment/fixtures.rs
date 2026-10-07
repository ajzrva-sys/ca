use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::workspace_materialization::{
    materialize_workspace, ClonePreference, MaterializationPolicy,
};

#[path = "../../../tests/reasoning_pilot_hidden/src/lib.rs"]
mod hidden_verifier;

pub const FIXTURE_IDS: [&str; 6] = hidden_verifier::FIXTURE_IDS;
pub const FIXTURE_CHECK_SCHEMA_VERSION: &str = "reasoning_pilot_fixture_check.v1";
const MAX_CANDIDATE_RECEIPT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FixtureCheckReceiptV1 {
    pub schema_version: String,
    pub passed: bool,
    pub network_isolation: String,
    pub candidates: Vec<FixtureCandidateReceiptV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FixtureCandidateReceiptV1 {
    pub fixture_id: String,
    pub kind: String,
    pub public_passed: bool,
    pub hidden_passed: bool,
    pub hidden_failure_code: Option<String>,
}

/// Internal one-candidate operation. It is invoked only by the feature
/// binary's session-isolated fixture worker and deliberately does not perform
/// any workspace creation, provider access, or cleanup.
pub fn verify_materialized_fixture(
    repository_root: &Path,
    fixture_id: &str,
    kind: &str,
    materialized: &Path,
) -> Result<FixtureCandidateReceiptV1> {
    let candidate = hidden_verifier::verify_materialized_fixture(
        repository_root,
        fixture_id,
        kind,
        materialized,
    )
    .map_err(|error| anyhow!(error))?;
    Ok(FixtureCandidateReceiptV1 {
        fixture_id: candidate.fixture_id,
        kind: candidate.kind,
        public_passed: candidate.public_passed,
        hidden_passed: candidate.hidden_passed,
        hidden_failure_code: candidate.hidden_failure_code,
    })
}

/// Verifies one controller-owned, mutated trial workspace. Unlike fixture
/// characterization, `trial` has no expected failure classification: callers
/// decide success solely from the public and hidden verifier booleans.
pub fn verify_trial_workspace(
    repository_root: &Path,
    fixture_id: &str,
    materialized: &Path,
) -> Result<FixtureCandidateReceiptV1> {
    verify_materialized_fixture(repository_root, fixture_id, "trial", materialized)
}

pub fn fixture_check(repository_root: &Path) -> Result<FixtureCheckReceiptV1> {
    let repository_root = repository_root
        .canonicalize()
        .map_err(|error| anyhow!("invalid fixture repository root: {error}"))?;
    let private_root = create_materialization_root(&repository_root)?;
    let artifact_root =
        create_private_temporary_root(&repository_root, "ca-rnd-097-fixture-artifacts-")?;
    let mut candidates = Vec::with_capacity(FIXTURE_IDS.len() * 2);
    let run = (|| -> Result<()> {
        for (kind, relative_root) in [
            ("baseline", "tests/fixtures/reasoning_pilot"),
            ("golden", "tests/reasoning_pilot_golden"),
        ] {
            for fixture_id in FIXTURE_IDS {
                let source = repository_root.join(relative_root).join(fixture_id);
                hidden_verifier::validate_fixture_source(&source, fixture_id)
                    .map_err(|error| anyhow!(error))?;
                let label = format!("{kind}-{fixture_id}");
                let workspace = materialize_workspace(
                    &source,
                    &private_root,
                    &label,
                    &MaterializationPolicy::benchmark(),
                    ClonePreference::PreferClone,
                )
                .map_err(|error| anyhow!("cannot materialize {kind}/{fixture_id}: {error}"))?;
                let candidate = verify_candidate_in_isolated_process(
                    &repository_root,
                    fixture_id,
                    kind,
                    workspace.path(),
                )
                .map_err(|error| anyhow!("cannot verify {kind}/{fixture_id}: {error}"))?;
                let artifact =
                    artifact_root.join(format!("{kind}-{fixture_id}.no-op-recovery.json"));
                write_noop_recovery_artifact(
                    &artifact,
                    fixture_id,
                    kind,
                    workspace.receipt().destination_manifest_sha256.as_str(),
                )?;
                let recovery = workspace
                    .verify_recovery_artifact(&artifact)
                    .map_err(|error| {
                        anyhow!("cannot verify {kind}/{fixture_id} recovery: {error}")
                    })?;
                if recovery.final_manifest_sha256()
                    != workspace.receipt().destination_manifest_sha256
                {
                    // The fixture checker itself is an offline no-op against
                    // the candidate source. A changed workspace is a failed
                    // verification outcome, but it still has a concrete,
                    // owner-only artifact bound to it, so do not leak that
                    // disposable tree while reporting the failure.
                    workspace.cleanup_after_export(recovery).map_err(|error| {
                        anyhow!("cannot clean changed {kind}/{fixture_id} workspace: {error}")
                    })?;
                    return Err(anyhow!(
                        "{kind}/{fixture_id} changed its disposable workspace; fixture verification must leave a no-op recovery state"
                    ));
                }
                workspace.cleanup_after_export(recovery).map_err(|error| {
                    anyhow!("cannot clean {kind}/{fixture_id} workspace: {error}")
                })?;
                candidates.push(FixtureCandidateReceiptV1 {
                    fixture_id: candidate.fixture_id,
                    kind: candidate.kind,
                    public_passed: candidate.public_passed,
                    hidden_passed: candidate.hidden_passed,
                    hidden_failure_code: candidate.hidden_failure_code,
                });
            }
        }
        Ok(())
    })();
    if run.is_ok() {
        cleanup_empty_materialization_root(&private_root)?;
        cleanup_artifact_root(&artifact_root)?;
    }
    run?;
    if candidates.len() != FIXTURE_IDS.len() * 2 {
        return Err(anyhow!(
            "fixture check returned an incomplete candidate set"
        ));
    }
    let passed = candidates.iter().all(|candidate| {
        candidate.public_passed
            && if candidate.kind == "baseline" {
                !candidate.hidden_passed
                    && candidate.hidden_failure_code.as_deref()
                        == Some(expected_failure_code(&candidate.fixture_id))
            } else {
                candidate.hidden_passed && candidate.hidden_failure_code.is_none()
            }
    });
    Ok(FixtureCheckReceiptV1 {
        schema_version: FIXTURE_CHECK_SCHEMA_VERSION.to_string(),
        passed,
        network_isolation: "sandbox-exec-deny-network".to_string(),
        candidates,
    })
}

pub fn validate_fixture_check_receipt(receipt: &FixtureCheckReceiptV1) -> Result<()> {
    if receipt.schema_version != FIXTURE_CHECK_SCHEMA_VERSION
        || !receipt.passed
        || receipt.network_isolation != "sandbox-exec-deny-network"
        || receipt.candidates.len() != FIXTURE_IDS.len() * 2
    {
        return Err(anyhow!("fixture receipt is not a complete isolated check"));
    }
    let mut observed = std::collections::BTreeSet::new();
    for candidate in &receipt.candidates {
        if !FIXTURE_IDS.contains(&candidate.fixture_id.as_str())
            || !observed.insert((candidate.kind.clone(), candidate.fixture_id.clone()))
            || !candidate.public_passed
        {
            return Err(anyhow!("fixture receipt contains an invalid candidate"));
        }
        match candidate.kind.as_str() {
            "baseline"
                if !candidate.hidden_passed
                    && candidate.hidden_failure_code.as_deref()
                        == Some(expected_failure_code(&candidate.fixture_id)) => {}
            "golden" if candidate.hidden_passed && candidate.hidden_failure_code.is_none() => {}
            _ => return Err(anyhow!("fixture receipt has an invalid expected outcome")),
        }
    }
    for fixture_id in FIXTURE_IDS {
        for kind in ["baseline", "golden"] {
            if !observed.contains(&(kind.into(), fixture_id.into())) {
                return Err(anyhow!("fixture receipt is missing a candidate"));
            }
        }
    }
    Ok(())
}

fn verify_candidate_in_isolated_process(
    repository_root: &Path,
    fixture_id: &str,
    kind: &str,
    workspace: &Path,
) -> Result<FixtureCandidateReceiptV1> {
    let executable = std::env::current_exe()
        .map_err(|error| anyhow!("cannot resolve fixture worker executable: {error}"))?;
    let mut command = Command::new(executable);
    command
        .arg("fixture-candidate")
        .arg("--repo")
        .arg(repository_root)
        .arg("--fixture-id")
        .arg(fixture_id)
        .arg("--kind")
        .arg(kind)
        .arg("--workspace")
        .arg(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() >= 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    let output = command
        .output()
        .map_err(|error| anyhow!("cannot start isolated fixture worker: {error}"))?;
    if output.stdout.len() > MAX_CANDIDATE_RECEIPT_BYTES
        || output.stderr.len() > MAX_CANDIDATE_RECEIPT_BYTES
    {
        return Err(anyhow!("isolated fixture worker exceeded the output bound"));
    }
    if !output.status.success() {
        return Err(anyhow!(
            "isolated fixture worker failed for {kind}/{fixture_id} with status {:?}",
            output.status.code()
        ));
    }
    let receipt: FixtureCandidateReceiptV1 = serde_json::from_slice(&output.stdout)
        .map_err(|_| anyhow!("isolated fixture worker returned an invalid receipt"))?;
    if receipt.fixture_id != fixture_id || receipt.kind != kind {
        return Err(anyhow!("isolated fixture receipt has mismatched identity"));
    }
    Ok(receipt)
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct NoopRecoveryArtifact<'a> {
    schema_version: &'static str,
    fixture_id: &'a str,
    kind: &'a str,
    final_manifest_sha256: &'a str,
}

fn write_noop_recovery_artifact(
    path: &Path,
    fixture_id: &str,
    kind: &str,
    final_manifest_sha256: &str,
) -> Result<()> {
    let bytes = serde_json::to_vec(&NoopRecoveryArtifact {
        schema_version: "reasoning_pilot_fixture_noop_recovery.v1",
        fixture_id,
        kind,
        final_manifest_sha256,
    })?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(path)
        .map_err(|error| anyhow!("cannot create recovery artifact: {error}"))?;
    file.write_all(&bytes)
        .map_err(|error| anyhow!("cannot write recovery artifact: {error}"))?;
    file.sync_all()
        .map_err(|error| anyhow!("cannot sync recovery artifact: {error}"))?;
    Ok(())
}

fn create_materialization_root(repository_root: &Path) -> Result<PathBuf> {
    create_private_temporary_root(repository_root, "ca-rnd-097-fixtures-")
}

fn create_private_temporary_root(repository_root: &Path, prefix: &str) -> Result<PathBuf> {
    let temporary = std::env::temp_dir()
        .canonicalize()
        .map_err(|error| anyhow!("cannot resolve temporary directory: {error}"))?;
    if temporary.starts_with(repository_root) || repository_root.starts_with(&temporary) {
        return Err(anyhow!(
            "fixture workspace root would overlap the repository"
        ));
    }
    let root = temporary.join(format!("{prefix}{}", Uuid::new_v4()));
    fs::create_dir(&root)
        .map_err(|error| anyhow!("cannot create fixture workspace root: {error}"))?;
    #[cfg(unix)]
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
        .map_err(|error| anyhow!("cannot protect fixture workspace root: {error}"))?;
    Ok(root)
}

fn cleanup_empty_materialization_root(root: &Path) -> Result<()> {
    let receipts = root.join(".workspace-receipts");
    if receipts.exists() {
        fs::remove_dir(&receipts)
            .map_err(|error| anyhow!("fixture workspace receipts were not empty: {error}"))?;
    }
    fs::remove_dir(root)
        .map_err(|error| anyhow!("fixture workspace root was not empty: {error}"))?;
    Ok(())
}

fn cleanup_artifact_root(root: &Path) -> Result<()> {
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("fixture artifact root has no safe name"))?;
    if !name.starts_with("ca-rnd-097-fixture-artifacts-") {
        return Err(anyhow!("fixture artifact root has an unsafe name"));
    }
    for entry in
        fs::read_dir(root).map_err(|error| anyhow!("cannot read artifact root: {error}"))?
    {
        let entry = entry.map_err(|error| anyhow!("cannot read artifact entry: {error}"))?;
        let metadata = entry
            .metadata()
            .map_err(|error| anyhow!("cannot inspect artifact entry: {error}"))?;
        if !metadata.is_file() {
            return Err(anyhow!("fixture artifact root contains a non-file entry"));
        }
        fs::remove_file(entry.path())
            .map_err(|error| anyhow!("cannot remove recovery artifact: {error}"))?;
    }
    fs::remove_dir(root).map_err(|error| anyhow!("cannot remove artifact root: {error}"))?;
    Ok(())
}

pub(crate) fn expected_failure_code(fixture_id: &str) -> &str {
    match fixture_id {
        "unicode_parser_boundary" => "unicode_mid_scalar",
        "permission_scope_matching" => "component_prefix_escape",
        "protocol_tool_interleave" => "interleaved_call_state",
        "atomic_journal_recovery" => "partial_tail_recovery",
        "process_timeout_cleanup" => "descendant_survived_timeout",
        "config_migration" => "legacy_config_rejected",
        _ => "unknown_fixture",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_inventory_is_bounded_and_stable() {
        assert_eq!(FIXTURE_IDS.len(), 6);
        assert_eq!(FIXTURE_IDS[0], "unicode_parser_boundary");
        assert_eq!(FIXTURE_IDS[5], "config_migration");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn materialized_fixture_uses_external_target_and_verified_cleanup() {
        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap();
        let private_root = create_materialization_root(&repository_root).unwrap();
        let artifact_root =
            create_private_temporary_root(&repository_root, "ca-rnd-097-fixture-artifacts-")
                .unwrap();
        let source = repository_root.join("tests/fixtures/reasoning_pilot/unicode_parser_boundary");
        hidden_verifier::validate_fixture_source(&source, "unicode_parser_boundary").unwrap();
        let workspace = materialize_workspace(
            &source,
            &private_root,
            "fixture-cleanup-test",
            &MaterializationPolicy::benchmark(),
            ClonePreference::PreferClone,
        )
        .unwrap();
        let candidate = hidden_verifier::verify_materialized_fixture(
            &repository_root,
            "unicode_parser_boundary",
            "baseline",
            workspace.path(),
        )
        .unwrap();
        assert!(candidate.public_passed);
        assert!(!candidate.hidden_passed);
        let artifact = artifact_root.join("unicode.no-op-recovery.json");
        write_noop_recovery_artifact(
            &artifact,
            "unicode_parser_boundary",
            "baseline",
            &workspace.receipt().destination_manifest_sha256,
        )
        .unwrap();
        let recovery = workspace.verify_recovery_artifact(&artifact).unwrap();
        assert_eq!(
            recovery.final_manifest_sha256(),
            workspace.receipt().destination_manifest_sha256
        );
        workspace.cleanup_after_export(recovery).unwrap();
        cleanup_empty_materialization_root(&private_root).unwrap();
        cleanup_artifact_root(&artifact_root).unwrap();
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn trial_verifier_accepts_a_fixed_isolated_workspace() {
        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap();
        let private_root = create_materialization_root(&repository_root).unwrap();
        let artifact_root =
            create_private_temporary_root(&repository_root, "ca-rnd-097-fixture-artifacts-")
                .unwrap();
        let source = repository_root.join("tests/reasoning_pilot_golden/unicode_parser_boundary");
        let workspace = materialize_workspace(
            &source,
            &private_root,
            "trial-verifier-test",
            &MaterializationPolicy::benchmark(),
            ClonePreference::PreferClone,
        )
        .unwrap();
        let candidate = verify_trial_workspace(
            &repository_root,
            "unicode_parser_boundary",
            workspace.path(),
        )
        .unwrap();
        assert!(candidate.public_passed);
        assert!(candidate.hidden_passed);
        assert_eq!(candidate.kind, "trial");
        let artifact = artifact_root.join("unicode.trial-recovery.json");
        write_noop_recovery_artifact(
            &artifact,
            "unicode_parser_boundary",
            "trial",
            &workspace.receipt().destination_manifest_sha256,
        )
        .unwrap();
        let recovery = workspace.verify_recovery_artifact(&artifact).unwrap();
        workspace.cleanup_after_export(recovery).unwrap();
        cleanup_empty_materialization_root(&private_root).unwrap();
        cleanup_artifact_root(&artifact_root).unwrap();
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn fixture_verifier_requires_the_supported_network_sandbox() {
        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap();
        let materialized = tempfile::tempdir().unwrap();
        let error = verify_materialized_fixture(
            &repository_root,
            "unicode_parser_boundary",
            "baseline",
            materialized.path(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("requires macOS sandbox-exec network isolation"));
    }

    #[test]
    #[ignore = "runs the complete external fixture and hidden-verifier suite"]
    fn complete_fixture_check_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let receipt = fixture_check(root).expect("fixture check should run");
        assert!(receipt.passed, "{receipt:#?}");
    }
}
