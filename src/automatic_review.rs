//! A host-owned finalization barrier. Child prose never establishes edit scope.
use crate::agent::subagent::{
    AgentContextMode, AgentGrant, AgentIsolation, AgentManager, AgentRunOrigin, AgentRunPurpose,
    AgentSpawnRequest, AgentStatus,
};
use crate::undo::{FileMutationBatchReceipt, FileMutationReceipt, FileState};
use crate::workspace::WorkspaceContext;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

const MAX_FILES: usize = 256;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const POLICY: &str = "ca.automatic-review.v1:independent-readonly-exact-candidate";

#[derive(Debug, Clone, Default)]
pub(crate) struct ReviewTracker {
    turn: Option<Uuid>,
    files: BTreeMap<PathBuf, FileMutationReceipt>,
    typed_after_hashes: BTreeMap<PathBuf, std::collections::BTreeSet<[u8; 32]>>,
    child_receipts_seen: std::collections::BTreeSet<String>,
    error: Option<String>,
    pub pending: bool,
    pub approved_digest: Option<String>,
    pub attempts: u8,
    pub finalization_ready: bool,
    pub verification_receipt: Option<crate::mechanistic_delegation::AutomaticVerificationOutcome>,
    accepted_verification_receipt:
        Option<crate::mechanistic_delegation::AutomaticVerificationOutcome>,
}

#[derive(Debug, Clone)]
pub(crate) struct ReviewCandidate {
    pub turn: Uuid,
    pub digest: String,
    files: Vec<FileMutationReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReviewFinding {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReviewReport {
    pub approved: bool,
    pub findings: Vec<ReviewFinding>,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ReviewOutcome {
    pub turn: Uuid,
    pub digest: String,
    pub result: Result<ReviewReport, String>,
    pub verification: Option<crate::mechanistic_delegation::AutomaticVerificationOutcome>,
}

impl ReviewOutcome {
    pub(crate) async fn acknowledge_receipts(&self, manager: &AgentManager) {
        // Errors can precede collection, including a writer-settlement timeout.
        // Retain those source receipts for retry; collected clones are idempotent.
        if self.result.is_ok() {
            let _ = manager.take_mutations_for_turn(self.turn).await;
        }
    }
}

#[derive(Clone)]
pub(crate) struct ReviewAuthority {
    pub verification_enabled: bool,
    pub verification_timeout: std::time::Duration,
    pub verification_backend: Option<Arc<crate::mcp::McpManager>>,
    pub allow_unisolated_evidence: bool,
    pub allow_unscoped_full: bool,
    pub job_owner: crate::jobs::JobOwner,
    pub approval: crate::app::ApprovalMode,
    pub approval_rules: Vec<crate::config::ApprovalRule>,
    pub permission_rules: Vec<crate::permission::PermissionRule>,
    pub session_grants: Vec<crate::permission::PermissionMatcher>,
    pub tool_policy: crate::tool_selection::ToolSelectionPolicy,
}

impl ReviewTracker {
    pub fn accept(&mut self, digest: String) {
        self.approved_digest = Some(digest);
        self.accepted_verification_receipt = self.verification_receipt.clone();
    }
    fn record_child(&mut self, turn: Uuid, receipt: &crate::agent::subagent::AgentMutationReceipt) {
        self.begin(turn);
        // Retried snapshots clone a physical receipt's identity. Distinct
        // writes can have equal contents after an edit/revert/edit sequence.
        if self.child_receipts_seen.insert(format!(
            "{}:{}",
            receipt.agent_id,
            receipt.mutation.persisted_id()
        )) {
            if receipt.aggregate {
                self.record_aggregate(turn, &receipt.mutation);
            } else {
                self.record(turn, &receipt.mutation);
            }
        }
    }

    pub fn consume_finalization(&mut self, workspace: &WorkspaceContext) -> Result<bool, String> {
        if !std::mem::take(&mut self.finalization_ready) {
            return Ok(false);
        }
        match self.candidate(workspace)? {
            Some(candidate) => {
                candidate.validate_disk(workspace)?;
                Ok(self.approved_digest.as_deref() == Some(candidate.digest.as_str()))
            }
            None => Ok(self.approved_digest.as_deref() == Some("no_changes")),
        }
    }
    pub fn begin(&mut self, turn: Uuid) {
        if self.turn != Some(turn) {
            *self = Self {
                turn: Some(turn),
                ..Default::default()
            };
        }
    }

    pub fn record(&mut self, turn: Uuid, batch: &FileMutationBatchReceipt) {
        self.begin(turn);
        self.finalization_ready = false;
        for receipt in batch.receipts() {
            if !reviewable(&receipt.path, &receipt.before, &receipt.after) {
                continue;
            }
            self.typed_after_hashes
                .entry(receipt.path.clone())
                .or_default()
                .insert(state_hash(&receipt.after));
            match self.files.get_mut(&receipt.path) {
                Some(previous) => {
                    if previous.after != receipt.before {
                        self.error =
                            Some("edit ownership changed between recorded mutations".into());
                    }
                    previous.after = receipt.after.clone();
                }
                None => {
                    self.files.insert(receipt.path.clone(), receipt.clone());
                }
            }
        }
        self.files.retain(|_, file| file.before != file.after);
        let bytes: usize = self
            .files
            .values()
            .map(|file| state_bytes(&file.before) + state_bytes(&file.after))
            .sum();
        if self.files.len() > MAX_FILES || bytes > MAX_BYTES {
            self.error = Some("automatic review mutation evidence exceeds its bound".into());
        }
        // An identical/no-op receipt must not discard the accepted candidate.
    }

    /// Shell snapshots span intermediate typed edits already recorded by tools.
    pub fn record_aggregate(&mut self, turn: Uuid, batch: &FileMutationBatchReceipt) {
        self.begin(turn);
        self.finalization_ready = false;
        for receipt in batch.receipts() {
            if !reviewable(&receipt.path, &receipt.before, &receipt.after) {
                continue;
            }
            match self.files.get_mut(&receipt.path) {
                Some(previous) => {
                    if !self
                        .typed_after_hashes
                        .get(&receipt.path)
                        .is_some_and(|states| states.contains(&state_hash(&receipt.before)))
                    {
                        previous.before = receipt.before.clone();
                    }
                    previous.after = receipt.after.clone();
                }
                None => {
                    self.files.insert(receipt.path.clone(), receipt.clone());
                }
            }
        }
        self.files.retain(|_, file| file.before != file.after);
        let bytes: usize = self
            .files
            .values()
            .map(|file| state_bytes(&file.before) + state_bytes(&file.after))
            .sum();
        if self.files.len() > MAX_FILES || bytes > MAX_BYTES {
            self.error = Some("automatic review mutation evidence exceeds its bound".into());
        }
    }

    pub fn candidate(
        &self,
        workspace: &WorkspaceContext,
    ) -> Result<Option<ReviewCandidate>, String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if self.files.is_empty() {
            return Ok(None);
        }
        let turn = self.turn.ok_or("automatic review has no parent turn")?;
        let mut hash = Sha256::new();
        hash.update(POLICY);
        hash.update(turn.as_bytes());
        for root in workspace.roots() {
            hash.update(root.id.to_string());
            hash.update(root.path.as_os_str().as_encoded_bytes());
        }
        for file in self.files.values() {
            hash.update(file.path.as_os_str().as_encoded_bytes());
            for state in [&file.before, &file.after] {
                match state {
                    FileState::Missing => hash.update(b"missing\0"),
                    FileState::Utf8(text) => {
                        hash.update(b"present\0");
                        hash.update(Sha256::digest(text.as_bytes()));
                    }
                }
            }
        }
        Ok(Some(ReviewCandidate {
            turn,
            digest: format!("{:x}", hash.finalize()),
            files: self.files.values().cloned().collect(),
        }))
    }
}

fn state_bytes(state: &FileState) -> usize {
    match state {
        FileState::Missing => 0,
        FileState::Utf8(text) => text.len(),
    }
}
fn state_hash(state: &FileState) -> [u8; 32] {
    let mut hash = Sha256::new();
    match state {
        FileState::Missing => hash.update(b"missing\0"),
        FileState::Utf8(text) => {
            hash.update(b"utf8\0");
            hash.update(text.as_bytes());
        }
    }
    hash.finalize().into()
}

fn reviewable(path: &Path, before: &FileState, after: &FileState) -> bool {
    if before == after {
        return false;
    }
    if matches!(
        path.file_name().and_then(|part| part.to_str()),
        Some("requirements.txt" | "CMakeLists.txt" | "constraints.txt")
    ) {
        return true;
    }
    let extension = path
        .extension()
        .and_then(|part| part.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "md" | "rst" | "adoc") {
        return false;
    }
    if extension == "txt" {
        let name = path
            .file_stem()
            .and_then(|part| part.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(
            name.as_str(),
            "readme" | "license" | "changelog" | "news" | "authors"
        ) || path
            .components()
            .any(|part| matches!(part.as_os_str().to_str(), Some("docs" | "documentation")))
        {
            return false;
        }
    }
    !matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "pdf" | "mp3" | "mp4"
    )
}

impl ReviewCandidate {
    pub fn validate_disk(&self, workspace: &WorkspaceContext) -> Result<(), String> {
        for file in &self.files {
            let resolved = workspace
                .resolve(
                    &file.path.to_string_lossy(),
                    crate::workspace::PathOperation::Create,
                )
                .map_err(|_| {
                    "review candidate path no longer has its original workspace authority"
                })?;
            if resolved != file.path {
                return Err("review candidate path changed through a symlink".into());
            }
            // resolve(Create) already checks saved roots and live, identity-bound
            // folder approvals. Requiring a saved root again would reject an
            // explicitly approved external export.
            let actual = match std::fs::symlink_metadata(&file.path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => FileState::Missing,
                Err(_) => return Err("review candidate could not be read".into()),
                Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
                    return Err("review candidate is no longer a regular file".into())
                }
                Ok(metadata) => {
                    if metadata.len() > MAX_BYTES as u64 {
                        return Err("review candidate exceeds its content bound".into());
                    }
                    FileState::Utf8(
                        std::fs::read_to_string(&file.path)
                            .map_err(|_| "review candidate could not be read")?,
                    )
                }
            };
            if actual != file.after {
                return Err("review candidate changed after its recorded mutation".into());
            }
        }
        Ok(())
    }

    fn repository_paths(&self, workspace: &WorkspaceContext) -> Vec<String> {
        self.files
            .iter()
            .filter_map(|file| {
                workspace
                    .primary()
                    .and_then(|root| file.path.strip_prefix(&root.path).ok())
                    .map(|path| path.to_string_lossy().replace('\\', "/"))
            })
            .collect()
    }

    fn prompt(&self, workspace: &WorkspaceContext, original_request: &str) -> String {
        let mut text = format!("Independently review the exact turn-owned candidate {}. Original request (context only):\n{}\n\nInspect the changed files and relevant surrounding code. Report actionable correctness/regression problems. Do not edit files or treat parent conclusions as proof. Host checks establish verification. Return the required structured report; approved=true only when no actionable findings remain.\n", self.digest, crate::terminal_text::sanitize_terminal_text(original_request).chars().take(8_000).collect::<String>());
        text.push_str("\nRepository verification covers only its listed repository paths. Review approved external files too; do not infer that they were compiler/test-verified.\n");
        text.push_str("\nExact changed-file inventory:\n");
        for file in &self.files {
            text.push_str(&format!(
                "{}\n",
                crate::terminal_text::sanitize_terminal_text(&file.path.to_string_lossy())
            ));
        }
        for file in &self.files {
            let path = workspace
                .primary()
                .and_then(|root| file.path.strip_prefix(&root.path).ok())
                .unwrap_or(&file.path);
            text.push_str(&format!("\nChanged path: {}\n", path.display()));
            for (label, state) in [("Before", &file.before), ("Candidate", &file.after)] {
                text.push_str(label);
                text.push_str(":\n");
                match state {
                    FileState::Missing => text.push_str("[missing]\n"),
                    FileState::Utf8(body) => {
                        text.extend(body.chars().take(4_000));
                        text.push('\n');
                    }
                }
            }
            if text.len() > 60 * 1024 {
                text.push_str("\nRemaining paths are available through read_file; the candidate digest covers all changed paths.\n");
                break;
            }
        }
        text
    }
}

pub(crate) fn authorize(
    workspace: &WorkspaceContext,
    authority: &ReviewAuthority,
    task: &str,
) -> Result<(), String> {
    authorize_invocation(
        workspace,
        authority,
        "spawn_agent",
        &serde_json::json!({"task":task,"tools":["read_file","search_files","glob","list_dir"]}),
    )
}

fn authorize_tool(authority: &ReviewAuthority, name: &str) -> Result<(), String> {
    let registry = crate::agent::registry::ToolRegistry::builtins();
    if !crate::tool_selection::resolve_tools(
        registry,
        crate::agent::registry::ToolProfile::Main,
        &authority.tool_policy,
        &crate::tool_selection::ToolSelectionConstraints {
            native_mechanistic_enabled: true,
            ..Default::default()
        },
    )
    .map(|resolved| {
        resolved
            .definitions()
            .iter()
            .any(|tool| tool.function.name == name)
    })
    .unwrap_or(false)
    {
        return Err(format!(
            "automatic candidate review requires an authorized {name} tool"
        ));
    }
    Ok(())
}

pub(crate) fn authorize_invocation(
    workspace: &WorkspaceContext,
    authority: &ReviewAuthority,
    name: &str,
    arguments: &serde_json::Value,
) -> Result<(), String> {
    authorize_tool(authority, name)?;
    let (_, decision) = crate::approval::ApprovalBroker::with_policy(
        workspace,
        &authority.approval_rules,
        &authority.permission_rules,
        &authority.session_grants,
        crate::approval::ApprovalOrigin::Review,
    )
    .prepare(name, arguments, crate::agent::registry::ToolOrigin::BuiltIn)?;
    match decision {
        crate::approval::BrokerDecision::AllowedByRule => Ok(()),
        crate::approval::BrokerDecision::Prompt if authority.approval == crate::app::ApprovalMode::RunEverything || (authority.approval == crate::app::ApprovalMode::AutoReview && crate::permission::auto_review_safe_invocation(name, arguments)) => Ok(()),
        crate::approval::BrokerDecision::DeniedByRule => Err(format!("automatic candidate {name} was denied by the active permission rules")),
        _ => Err(format!("automatic candidate {name} requires interactive approval; permit the exact invocation through /permissions and retry")),
    }
}

struct ChildGuard {
    manager: Arc<AgentManager>,
    id: crate::agent::subagent::AgentId,
    finished: bool,
}

fn authorize_candidate_reads(
    candidate: &ReviewCandidate,
    workspace: &WorkspaceContext,
    authority: &ReviewAuthority,
) -> Result<(), String> {
    for file in &candidate.files {
        let args = serde_json::json!({"path":file.path.to_string_lossy()});
        if matches!(file.after, FileState::Missing) {
            // Deleted bytes exist only in the host-owned receipt. Authorize
            // their canonical destination under the same read rules without
            // pretending a physical read of the deleted file can succeed.
            let path = workspace
                .resolve(
                    &file.path.to_string_lossy(),
                    crate::workspace::PathOperation::Create,
                )
                .map_err(|error| error.to_string())?;
            authorize_tool(authority, "read_file")?;
            let args = serde_json::json!({"path":path.to_string_lossy()});
            match crate::permission::evaluate(
                "read_file",
                &args,
                &authority.approval_rules,
                &authority.permission_rules,
                &authority.session_grants,
            ) {
                crate::permission::PermissionOutcome::Deny => {
                    return Err(
                        "automatic candidate read_file was denied by the active permission rules"
                            .into(),
                    )
                }
                crate::permission::PermissionOutcome::Allow => {}
                crate::permission::PermissionOutcome::NoMatch
                    if matches!(
                        authority.approval,
                        crate::app::ApprovalMode::RunEverything
                            | crate::app::ApprovalMode::AutoReview
                    ) => {}
                _ => {
                    return Err(
                        "automatic candidate read_file requires interactive approval".into(),
                    )
                }
            }
        } else {
            authorize_invocation(workspace, authority, "read_file", &args)?;
        }
    }
    Ok(())
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.finished {
            let manager = self.manager.clone();
            let id = self.id;
            tokio::spawn(async move {
                let _ = manager.cancel(id).await;
            });
        }
    }
}

pub(crate) async fn run(
    manager: Arc<AgentManager>,
    candidate: ReviewCandidate,
    workspace: WorkspaceContext,
    original_request: String,
    authority: ReviewAuthority,
) -> ReviewOutcome {
    let turn = candidate.turn;
    let digest = candidate.digest.clone();
    let result=async {
        let task="Independently review the final code/configuration candidate";
        authorize(&workspace,&authority,task)?;
        authorize_candidate_reads(&candidate, &workspace, &authority)?;
        if manager.spawns_paused() { return Err("automatic review is paused with subagent spawning".into()); }
        let settle=async {
            loop {
                if manager.subscribe_activity().borrow().iter().all(|agent| agent.origin==AgentRunOrigin::AutomaticReview || agent.status.terminal() || agent.parent_turn!=turn) { return Ok::<(),String>(()); }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(1800),settle).await.map_err(|_| "automatic review timed out waiting for child writers")??;
        let check=candidate.clone();let roots=workspace.clone();
        tokio::task::spawn_blocking(move || check.validate_disk(&roots)).await.map_err(|_| "review candidate worker stopped")??;
        let schema=serde_json::json!({"type":"object","additionalProperties":false,"required":["approved","findings","summary"],"properties":{"approved":{"type":"boolean"},"summary":{"type":"string","maxLength":8000},"findings":{"type":"array","maxItems":32,"items":{"type":"object","additionalProperties":false,"required":["path","message"],"properties":{"path":{"type":"string","maxLength":1024},"message":{"type":"string","maxLength":8000}}}}}});
        let contract=crate::headless_contract::StructuredOutputContract::from_bytes(&serde_json::to_vec(&schema).map_err(|_| "review schema encoding failed")?)?;
        let route=manager.default_delegation_route_for_turn(turn);
        let id=manager.spawn_with_origin(AgentSpawnRequest { parent:None,parent_turn:turn,task_id:None,task:task.into(),context_mode:AgentContextMode::Summary,context_summary:candidate.prompt(&workspace,&original_request),grants:AgentGrant { tools:vec!["read_file".into(),"search_files".into(),"glob".into(),"list_dir".into()],workspace_root_ids:workspace.roots().iter().map(|root|root.id.to_string()).collect(),path_scopes:Vec::new(),command_scopes:Vec::new(),network:false },workspace:workspace.clone(),depth:1,route,purpose:AgentRunPurpose::Subagent,output_contract:Some(contract),isolation:AgentIsolation::Shared },AgentRunOrigin::AutomaticReview).await.map_err(|error|error.to_string())?;
        let _ = manager.set_role(id, "Reviewer").await;
        let mut guard=ChildGuard { manager:manager.clone(),id,finished:false };
        let snapshot=manager.await_agent(id).await.map_err(|error|error.to_string())?;
        guard.finished=true;
        if snapshot.status!=AgentStatus::Completed { return Err(format!("independent reviewer finished without approval: {:?}",snapshot.status)); }
        let value=snapshot.structured_result.ok_or("independent reviewer did not return a validated report")?;
        let mut report:ReviewReport=serde_json::from_value(value).map_err(|_|"invalid independent review report")?;
        report.approved &= report.findings.is_empty();
        let check=candidate;let roots=workspace;
        tokio::task::spawn_blocking(move || check.validate_disk(&roots)).await.map_err(|_| "review candidate worker stopped")??;
        Ok(report)
    }.await;
    ReviewOutcome {
        turn,
        digest,
        result,
        verification: None,
    }
}

async fn native_verification_revision(
    workspace: &WorkspaceContext,
) -> Result<Option<String>, String> {
    match crate::mechanistic_delegation::native_repository_revision(workspace).await {
        Ok(revision) => Ok(Some(revision)),
        Err(error) => {
            let repository_expected = workspace.primary().is_some_and(|root| {
                root.path
                    .ancestors()
                    .any(|path| std::fs::symlink_metadata(path.join(".git")).is_ok())
            });
            if repository_expected {
                Err(format!(
                    "final candidate repository verification is unavailable: {error}"
                ))
            } else {
                // Standalone workspaces have no repository verification recipe.
                // Their independent read-only candidate review still runs.
                Ok(None)
            }
        }
    }
}

pub(crate) async fn run_barrier(
    manager: Arc<AgentManager>,
    mut tracker: ReviewTracker,
    turn: Uuid,
    workspace: WorkspaceContext,
    original_request: String,
    authority: ReviewAuthority,
) -> (ReviewTracker, ReviewOutcome) {
    let result = async {
        let wait = async {
            loop {
                if manager.subscribe_activity().borrow().iter().all(|agent| {
                    agent.parent_turn != turn || agent.origin == AgentRunOrigin::AutomaticReview || agent.status.terminal()
                }) { break; }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(1800), wait).await.map_err(|_| "review timed out waiting for turn-owned writers")?;
        for child in manager.snapshot_mutations_for_turn(turn).await? {
            tracker.record_child(turn, &child);
        }
        for batch in crate::command_mutations::preview(authority.job_owner).await? { tracker.record_aggregate(turn,&batch); }
        let Some(candidate) = tracker.candidate(&workspace)? else {
            return Ok(ReviewOutcome { turn, digest: "no_changes".into(), verification:None,result: Ok(ReviewReport { approved: true, findings: Vec::new(), summary: "No net executable changes require review".into() }) });
        };
        if tracker.approved_digest.as_deref() == Some(candidate.digest.as_str()) {
            let check=candidate.clone();let roots=workspace.clone();
            tokio::task::spawn_blocking(move||check.validate_disk(&roots)).await.map_err(|_|"review candidate worker stopped")??;
            tracker.verification_receipt = tracker.accepted_verification_receipt.clone();
            return Ok(ReviewOutcome { turn, digest: candidate.digest,verification:tracker.accepted_verification_receipt.clone(), result: Ok(ReviewReport { approved:true, findings:Vec::new(),summary:"Reused exact independent candidate review".into() }) });
        }
        if tracker.attempts >= 3 { return Err("independent review exhausted its three candidate attempts".into()); }
        authorize(&workspace,&authority,"Independently review the final code/configuration candidate")?;
        if manager.spawns_paused() { return Err("automatic review is paused with subagent spawning".into()); }
        tracker.attempts += 1;
        tracker.verification_receipt=None;
        if authority.verification_enabled {
            let check=candidate.clone();let roots=workspace.clone();
            tokio::task::spawn_blocking(move||check.validate_disk(&roots)).await.map_err(|_|"candidate verification worker stopped")??;
            let external=authority.verification_backend.clone();
            let revision=native_verification_revision(&workspace).await?;
            let paths=candidate.repository_paths(&workspace);
            if !paths.is_empty() && (external.is_some() || revision.is_some()) {
                authorize_invocation(&workspace,&authority,"verify_changes",&serde_json::json!({"paths":paths}))?;
                let outcome=crate::mechanistic_delegation::candidate_verification(&workspace,external,paths,authority.verification_timeout,authority.allow_unisolated_evidence,authority.allow_unscoped_full).await;
                tracker.verification_receipt=Some(outcome.clone());
                if matches!(outcome.status,crate::mechanistic_delegation::AutomaticVerificationStatus::Failed) {
                    return Ok(ReviewOutcome{turn,digest:candidate.digest,verification:Some(outcome.clone()),result:Ok(ReviewReport{approved:false,findings:vec![ReviewFinding{path:"host verification".into(),message:outcome.context.unwrap_or_else(||"The exact final candidate did not pass shared-engine verification".into())}],summary:"Final candidate verification remains incomplete".into()})});
                }
            }
        }
        let mut outcome=run(manager, candidate, workspace, original_request, authority).await;
        outcome.verification=tracker.verification_receipt.clone();
        Ok(outcome)
    }.await;
    tracker.pending = false;
    let outcome = result.unwrap_or_else(|error: String| ReviewOutcome {
        turn,
        digest: String::new(),
        verification: None,
        result: Err(error),
    });
    (tracker, outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temporary_root() -> tempfile::TempDir {
        tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap()
    }
    fn receipt(path: &Path, before: &str, after: &str) -> FileMutationBatchReceipt {
        FileMutationBatchReceipt::single(FileMutationReceipt::new(
            path.into(),
            "write_file",
            FileState::Utf8(before.into()),
            FileState::Utf8(after.into()),
        ))
    }

    #[test]
    fn hrs293_live_folder_approval_is_valid_review_scope_and_revocation_is_not() {
        let root = temporary_root();
        let project = root.path().join("project");
        let exports = root.path().join("exports");
        std::fs::create_dir(&project).unwrap();
        std::fs::create_dir(&exports).unwrap();
        let mut workspace = WorkspaceContext::discover(&project).unwrap();
        workspace
            .grant_session_write(crate::workspace::SessionWriteRoot::prepare(&exports).unwrap())
            .unwrap();
        let path = exports.join("export.rs");
        std::fs::write(&path, "approved export").unwrap();
        let mut tracker = ReviewTracker::default();
        tracker.record(Uuid::new_v4(), &receipt(&path, "before", "approved export"));
        let candidate = tracker.candidate(&workspace).unwrap().unwrap();
        assert_eq!(candidate.validate_disk(&workspace), Ok(()));
        assert!(candidate.repository_paths(&workspace).is_empty());
        let code = project.join("code.rs");
        std::fs::write(&code, "changed source").unwrap();
        tracker.record(candidate.turn, &receipt(&code, "before", "changed source"));
        let mixed = tracker.candidate(&workspace).unwrap().unwrap();
        assert_eq!(mixed.validate_disk(&workspace), Ok(()));
        assert_eq!(mixed.repository_paths(&workspace), vec!["code.rs"]);
        workspace.clear_session_writes();
        assert!(candidate.validate_disk(&workspace).is_err());
    }
    fn verification(
        context: &str,
        status: crate::mechanistic_delegation::AutomaticVerificationStatus,
    ) -> crate::mechanistic_delegation::AutomaticVerificationOutcome {
        crate::mechanistic_delegation::AutomaticVerificationOutcome {
            status,
            context: Some(context.into()),
            job_id: None,
            repository_revision: None,
            receipt_digest: Some(context.into()),
            selected_paths: Vec::new(),
            selected_recipes: Vec::new(),
            reason_codes: Vec::new(),
            cache_state: None,
            duration_millis: 0,
        }
    }
    #[test]
    fn finalization_rechecks_exact_approved_bytes_after_pause() {
        let root = temporary_root();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let path = root.path().join("source.rs");
        std::fs::write(&path, "approved").unwrap();
        let mut tracker = ReviewTracker::default();
        tracker.record(Uuid::new_v4(), &receipt(&path, "before", "approved"));
        tracker.accept(tracker.candidate(&workspace).unwrap().unwrap().digest);
        tracker.finalization_ready = true;
        std::fs::write(&path, "changed while finalization paused").unwrap();
        assert!(tracker.consume_finalization(&workspace).is_err());
        assert!(!tracker.finalization_ready);
        std::fs::write(&path, "approved").unwrap();
        tracker.finalization_ready = true;
        assert!(tracker.consume_finalization(&workspace).unwrap());
    }
    #[test]
    fn retained_child_receipts_are_idempotent_on_cancelled_review_retry() {
        let root = temporary_root();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let path = root.path().join("source.rs");
        let turn = Uuid::new_v4();
        let child = crate::agent::subagent::AgentId::new();
        let first = crate::agent::subagent::AgentMutationReceipt {
            agent_id: child,
            parent_turn: turn,
            mutation: receipt(&path, "before", "first"),
            aggregate: false,
            aggregate_started_at: None,
        };
        let second = crate::agent::subagent::AgentMutationReceipt {
            mutation: receipt(&path, "first", "second"),
            ..first.clone()
        };
        let mut tracker = ReviewTracker::default();
        tracker.record_child(turn, &first);
        tracker.record_child(turn, &second);
        let candidate = tracker.candidate(&workspace).unwrap().unwrap();
        tracker.record_child(turn, &first);
        tracker.record_child(turn, &second);
        assert_eq!(
            tracker.candidate(&workspace).unwrap().unwrap().digest,
            candidate.digest
        );
    }
    #[test]
    fn repeated_child_write_after_revert_still_requires_review() {
        let root = temporary_root();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let path = root.path().join("source.rs");
        let turn = Uuid::new_v4();
        let first = crate::agent::subagent::AgentMutationReceipt {
            agent_id: crate::agent::subagent::AgentId::new(),
            parent_turn: turn,
            mutation: receipt(&path, "before", "changed"),
            aggregate: false,
            aggregate_started_at: None,
        };
        let revert = crate::agent::subagent::AgentMutationReceipt {
            mutation: receipt(&path, "changed", "before"),
            ..first.clone()
        };
        let repeated = crate::agent::subagent::AgentMutationReceipt {
            mutation: receipt(&path, "before", "changed"),
            ..first.clone()
        };
        assert_eq!(first.mutation.digest, repeated.mutation.digest);
        assert_ne!(
            first.mutation.persisted_id(),
            repeated.mutation.persisted_id()
        );
        let mut tracker = ReviewTracker::default();
        tracker.record_child(turn, &first);
        tracker.record_child(turn, &revert);
        assert!(tracker.candidate(&workspace).unwrap().is_none());
        tracker.record_child(turn, &repeated);
        let candidate = tracker.candidate(&workspace).unwrap().unwrap();
        assert_eq!(candidate.files[0].before, FileState::Utf8("before".into()));
        assert_eq!(candidate.files[0].after, FileState::Utf8("changed".into()));
        for cloned in [&first, &revert, &repeated] {
            tracker.record_child(turn, cloned);
        }
        assert_eq!(
            tracker.candidate(&workspace).unwrap().unwrap().digest,
            candidate.digest
        );
    }
    #[tokio::test]
    async fn corrupt_repository_keeps_final_verification_incomplete() {
        let root = temporary_root();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        assert!(native_verification_revision(&workspace)
            .await
            .unwrap()
            .is_none());
        std::fs::create_dir(root.path().join(".git")).unwrap();
        assert!(native_verification_revision(&workspace)
            .await
            .unwrap_err()
            .contains("verification is unavailable"));
    }
    #[test]
    fn accepted_verification_survives_another_failed_candidate() {
        let root = temporary_root();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let path = root.path().join("source.rs");
        let turn = Uuid::new_v4();
        let mut tracker = ReviewTracker::default();
        tracker.record(turn, &receipt(&path, "before", "A"));
        let digest = tracker.candidate(&workspace).unwrap().unwrap().digest;
        let accepted = verification(
            "A checked receipt",
            crate::mechanistic_delegation::AutomaticVerificationStatus::Passed,
        );
        tracker.verification_receipt = Some(accepted.clone());
        tracker.accept(digest.clone());
        tracker.record(turn, &receipt(&path, "A", "B"));
        tracker.verification_receipt = Some(verification(
            "B failed receipt",
            crate::mechanistic_delegation::AutomaticVerificationStatus::Failed,
        ));
        tracker.record(turn, &receipt(&path, "B", "A"));
        assert_eq!(
            tracker.candidate(&workspace).unwrap().unwrap().digest,
            digest
        );
        assert_eq!(
            tracker
                .accepted_verification_receipt
                .as_ref()
                .unwrap()
                .context,
            accepted.context
        );
    }
    #[test]
    fn review_tracks_only_net_owned_changes_and_invalidates_changed_candidates() {
        let root = temporary_root();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let path = root.path().join("main.rs");
        let turn = Uuid::new_v4();
        let mut tracker = ReviewTracker::default();
        tracker.record(turn, &receipt(&path, "original", "changed"));
        let first = tracker.candidate(&workspace).unwrap().unwrap();
        tracker.approved_digest = Some(first.digest.clone());
        tracker.record(turn, &receipt(&path, "changed", "changed"));
        assert_eq!(
            tracker.candidate(&workspace).unwrap().unwrap().digest,
            first.digest
        );
        tracker.record(turn, &receipt(&path, "changed", "fixed"));
        assert_ne!(
            tracker.candidate(&workspace).unwrap().unwrap().digest,
            first.digest
        );
        tracker.record(turn, &receipt(&path, "fixed", "original"));
        assert!(tracker.candidate(&workspace).unwrap().is_none());
    }
    #[test]
    fn prose_skips_but_snapshots_config_and_executable_examples_require_review() {
        for name in [
            "tests/result.snap",
            "config.toml",
            "src/main.rs",
            "script.sh",
            "tests/fixture.txt",
            "runtime/settings.txt",
        ] {
            assert!(reviewable(
                Path::new(name),
                &FileState::Missing,
                &FileState::Utf8("changed".into())
            ));
        }
        assert!(!reviewable(
            Path::new("README.md"),
            &FileState::Missing,
            &FileState::Utf8("prose".into())
        ));
        assert!(!reviewable(
            Path::new("docs/guide.txt"),
            &FileState::Missing,
            &FileState::Utf8("prose".into())
        ));
        assert!(!reviewable(
            Path::new("README.md"),
            &FileState::Missing,
            &FileState::Utf8("```sh\nrun\n```".into())
        ));
        for name in ["requirements.txt", "CMakeLists.txt", "constraints.txt"] {
            assert!(reviewable(
                Path::new(name),
                &FileState::Missing,
                &FileState::Utf8("runtime change".into())
            ));
        }
    }
    #[test]
    fn final_verification_respects_explicit_denial_and_tool_exclusion() {
        let root = temporary_root();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let mut authority = ReviewAuthority {
            job_owner: crate::jobs::JobOwner::new(None),
            approval: crate::app::ApprovalMode::RunEverything,
            approval_rules: vec![crate::config::ApprovalRule::DenyTool(
                "verify_changes".into(),
            )],
            permission_rules: Vec::new(),
            session_grants: Vec::new(),
            tool_policy: Default::default(),
            verification_enabled: true,
            verification_timeout: std::time::Duration::from_secs(1),
            verification_backend: None,
            allow_unisolated_evidence: false,
            allow_unscoped_full: false,
        };
        let args = serde_json::json!({"paths":["source.rs"]});
        assert!(
            authorize_invocation(&workspace, &authority, "verify_changes", &args)
                .unwrap_err()
                .contains("denied")
        );
        authority.approval_rules.clear();
        authority.tool_policy =
            crate::tool_selection::ToolSelectionPolicy::replacing(&["read_file"]);
        assert!(authorize_invocation(&workspace, &authority, "verify_changes", &args).is_err());
        authority.tool_policy = Default::default();
        authority.approval = crate::app::ApprovalMode::Allowlist;
        assert!(
            authorize_invocation(&workspace, &authority, "verify_changes", &args)
                .unwrap_err()
                .contains("interactive approval")
        );
        authority.approval = crate::app::ApprovalMode::RunEverything;
        let path = root.path().join("source.rs");
        std::fs::write(&path, "candidate").unwrap();
        let turn = Uuid::new_v4();
        let mut tracker = ReviewTracker::default();
        tracker.record(turn, &receipt(&path, "before", "candidate"));
        authority.permission_rules = vec![crate::permission::PermissionRule {
            decision: crate::permission::PermissionDecision::Deny,
            matcher: crate::permission::PermissionMatcher::Path {
                tool: "read_file".into(),
                path_pattern: path.to_string_lossy().into_owned(),
            },
        }];
        assert!(authorize_candidate_reads(
            &tracker.candidate(&workspace).unwrap().unwrap(),
            &workspace,
            &authority
        )
        .unwrap_err()
        .contains("denied"));
        std::fs::remove_file(&path).unwrap();
        tracker.record(
            turn,
            &FileMutationBatchReceipt::single(FileMutationReceipt::new(
                path.clone(),
                "delete_file",
                FileState::Utf8("candidate".into()),
                FileState::Missing,
            )),
        );
        let deleted = tracker.candidate(&workspace).unwrap().unwrap();
        assert!(authorize_candidate_reads(&deleted, &workspace, &authority)
            .unwrap_err()
            .contains("denied"));
        authority.permission_rules.clear();
        assert!(authorize_candidate_reads(&deleted, &workspace, &authority).is_ok());
        authority.permission_rules = vec![crate::permission::PermissionRule {
            decision: crate::permission::PermissionDecision::Ask,
            matcher: crate::permission::PermissionMatcher::Tool {
                tool: "read_file".into(),
            },
        }];
        assert!(authorize_candidate_reads(&deleted, &workspace, &authority)
            .unwrap_err()
            .contains("interactive approval"));
    }
    #[test]
    fn disk_drift_and_receipt_discontinuity_fail_closed() {
        let root = temporary_root();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let path = root.path().join("main.rs");
        std::fs::write(&path, "changed").unwrap();
        let turn = Uuid::new_v4();
        let mut tracker = ReviewTracker::default();
        tracker.record(turn, &receipt(&path, "original", "changed"));
        let candidate = tracker.candidate(&workspace).unwrap().unwrap();
        candidate.validate_disk(&workspace).unwrap();
        std::fs::write(&path, "external").unwrap();
        assert!(candidate.validate_disk(&workspace).is_err());
        tracker.record(turn, &receipt(&path, "external", "fixed"));
        assert!(tracker.candidate(&workspace).is_err());
    }
    #[test]
    fn shell_aggregate_keeps_actual_initial_state_on_either_side_of_typed_edits() {
        let root = temporary_root();
        let path = root.path().join("main.rs");
        let turn = Uuid::new_v4();
        let mut shell_first = ReviewTracker::default();
        shell_first.record(turn, &receipt(&path, "shell intermediate", "typed final"));
        shell_first.record_aggregate(
            turn,
            &receipt(&path, "original dirty baseline", "typed final"),
        );
        assert_eq!(
            shell_first.files[&path].before,
            FileState::Utf8("original dirty baseline".into())
        );
        let mut typed_first = ReviewTracker::default();
        typed_first.record(
            turn,
            &receipt(&path, "original dirty baseline", "typed intermediate"),
        );
        typed_first.record_aggregate(turn, &receipt(&path, "typed intermediate", "shell final"));
        assert_eq!(
            typed_first.files[&path].before,
            FileState::Utf8("original dirty baseline".into())
        );
        assert_eq!(
            typed_first.files[&path].after,
            FileState::Utf8("shell final".into())
        );
    }
    #[cfg(unix)]
    #[test]
    fn parent_directory_symlink_drift_cannot_reuse_review() {
        let root = temporary_root();
        let outside = temporary_root();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let directory = root.path().join("src");
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("main.rs");
        std::fs::write(&path, "candidate").unwrap();
        let mut tracker = ReviewTracker::default();
        tracker.record(Uuid::new_v4(), &receipt(&path, "before", "candidate"));
        let candidate = tracker.candidate(&workspace).unwrap().unwrap();
        candidate.validate_disk(&workspace).unwrap();
        std::fs::write(outside.path().join("main.rs"), "candidate").unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
        std::os::unix::fs::symlink(outside.path(), &directory).unwrap();
        assert!(candidate.validate_disk(&workspace).is_err());
    }
}
