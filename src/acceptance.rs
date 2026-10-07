use crate::app::Message;
use crate::changes::ReviewInput;
use crate::review_convergence::{ReviewConvergenceRound, ReviewerVerdict};
use crate::runtime::{ActiveProfile, RuntimePaths};
use crate::work::{PlanId, WorkEventKind, WorkState};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const SCHEMA: &str = "ca.acceptance.v1";
const MAX_PACKET_BYTES: u64 = 256 * 1024;
const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PACKETS: usize = 256;
const MAX_PATHS: usize = 2048;
const MAX_EVENTS: usize = 512;
const MAX_CLAIMS: usize = 1024;
const MAX_RISKS: usize = 128;
const MAX_TEXT_BYTES: usize = 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcceptanceRevisionV1 {
    pub source_revision_sha256: String,
    pub workspace_revision_sha256: String,
    pub session_id: Option<Uuid>,
    pub session_revision_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcceptancePlanBindingV1 {
    pub id: PlanId,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceClaimLabel {
    Verified,
    Inferred,
    Unverified,
    Contradicted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcceptanceClaimV1 {
    pub label: AcceptanceClaimLabel,
    pub subject: String,
    pub evidence_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcceptanceWorkEventV1 {
    pub id: Uuid,
    pub kind: WorkEventKind,
    pub succeeded: bool,
    pub summary_sha256: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcceptanceReviewerV1 {
    pub provider: String,
    pub model: String,
    pub verdict: ReviewerVerdict,
    pub finding_count: usize,
    pub summary_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AcceptancePacketV1 {
    pub schema: String,
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub request_sha256: String,
    pub revision: AcceptanceRevisionV1,
    pub plan: Option<AcceptancePlanBindingV1>,
    pub changed_relative_paths: Vec<String>,
    pub agent_receipt_sha256: Vec<String>,
    pub work_events: Vec<AcceptanceWorkEventV1>,
    pub reviewers: Vec<AcceptanceReviewerV1>,
    pub reviewer_round: u8,
    pub reviewer_requests: u32,
    pub reviewer_output_bytes: usize,
    pub known_cost_usd: String,
    pub unpriced_requests: u32,
    pub unresolved_risks: Vec<String>,
    pub claims: Vec<AcceptanceClaimV1>,
    pub packet_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcceptanceSummaryV1 {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub passed: bool,
    pub stale: bool,
    pub reviewer_round: u8,
    pub packet_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceStore {
    root: PathBuf,
}

impl AcceptanceStore {
    pub fn for_paths(paths: &RuntimePaths, profile: &ActiveProfile) -> Self {
        Self::new(paths.acceptance_dir(profile))
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn create(&self, mut packet: AcceptancePacketV1) -> Result<PathBuf, String> {
        ensure_private_dir(&self.root)?;
        let existing = self.packet_paths()?;
        if existing.len() >= MAX_PACKETS {
            return Err(format!(
                "acceptance store reached its {MAX_PACKETS}-packet limit"
            ));
        }
        let total = existing.iter().try_fold(0_u64, |total, path| {
            let metadata = private_packet_metadata(path)?;
            total
                .checked_add(metadata.len())
                .ok_or_else(|| "acceptance store size overflowed".to_string())
        })?;
        packet.schema = SCHEMA.into();
        packet.packet_sha256.clear();
        validate_packet(&packet, false)?;
        packet.packet_sha256 = packet_sha256(&packet)?;
        let bytes = serde_json::to_vec_pretty(&packet)
            .map_err(|_| "acceptance packet could not be encoded".to_string())?;
        let byte_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if byte_len > MAX_PACKET_BYTES || total.saturating_add(byte_len) > MAX_TOTAL_BYTES {
            return Err("acceptance packet exceeds the private store limit".into());
        }
        let name = format!(
            "{}-{}-{}.json",
            packet.created_at.timestamp_millis(),
            packet.id.simple(),
            &packet.packet_sha256[..16]
        );
        let destination = self.root.join(name);
        if destination.exists() {
            return Err("acceptance packet destination already exists".into());
        }
        publish_private_immutable(&self.root, &destination, &bytes)?;
        Ok(destination)
    }

    pub fn list(
        &self,
        current: Option<&AcceptanceRevisionV1>,
    ) -> Result<Vec<AcceptanceSummaryV1>, String> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        validate_private_dir(&self.root)?;
        let mut packets = self
            .packet_paths()?
            .into_iter()
            .map(|path| self.load_path(&path))
            .collect::<Result<Vec<_>, _>>()?;
        packets.sort_by_key(|packet| std::cmp::Reverse((packet.created_at, packet.id)));
        Ok(packets
            .into_iter()
            .map(|packet| AcceptanceSummaryV1 {
                id: packet.id,
                created_at: packet.created_at,
                passed: packet
                    .reviewers
                    .iter()
                    .all(|reviewer| reviewer.verdict == ReviewerVerdict::Pass),
                stale: current.is_none_or(|revision| packet.revision != *revision),
                reviewer_round: packet.reviewer_round,
                packet_sha256: packet.packet_sha256,
            })
            .collect())
    }

    pub fn latest(&self) -> Result<Option<AcceptancePacketV1>, String> {
        let mut packets = self
            .packet_paths()?
            .into_iter()
            .map(|path| self.load_path(&path))
            .collect::<Result<Vec<_>, _>>()?;
        packets.sort_by_key(|packet| std::cmp::Reverse((packet.created_at, packet.id)));
        Ok(packets.into_iter().next())
    }

    pub fn export_latest(&self, output: &Path) -> Result<PathBuf, String> {
        let packet = self
            .latest()?
            .ok_or_else(|| "no acceptance packet is available".to_string())?;
        let bytes = serde_json::to_vec_pretty(&packet)
            .map_err(|_| "acceptance packet could not be encoded".to_string())?;
        let parent = output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or_else(|| "acceptance export requires an explicit parent directory".to_string())?;
        let parent = std::fs::canonicalize(parent)
            .map_err(|_| "acceptance export parent is unavailable".to_string())?;
        let name = output
            .file_name()
            .ok_or_else(|| "acceptance export path is invalid".to_string())?;
        let destination = parent.join(name);
        if destination.exists() {
            return Err("acceptance export refuses to overwrite an existing path".into());
        }
        publish_private_immutable(&parent, &destination, &bytes)?;
        Ok(destination)
    }

    fn load_path(&self, path: &Path) -> Result<AcceptancePacketV1, String> {
        let metadata = private_packet_metadata(path)?;
        if metadata.len() > MAX_PACKET_BYTES {
            return Err("acceptance packet exceeds its byte limit".into());
        }
        let mut options = OpenOptions::new();
        options.read(true).custom_flags(libc::O_NOFOLLOW);
        let file = options
            .open(path)
            .map_err(|_| "acceptance packet could not be opened safely".to_string())?;
        let mut bytes = Vec::new();
        file.take(MAX_PACKET_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "acceptance packet could not be read".to_string())?;
        if bytes.len() as u64 > MAX_PACKET_BYTES {
            return Err("acceptance packet exceeds its byte limit".into());
        }
        let packet: AcceptancePacketV1 = serde_json::from_slice(&bytes)
            .map_err(|_| "acceptance packet is malformed".to_string())?;
        validate_packet(&packet, true)?;
        Ok(packet)
    }

    fn packet_paths(&self) -> Result<Vec<PathBuf>, String> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        validate_private_dir(&self.root)?;
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(&self.root)
            .map_err(|_| "acceptance store could not be listed".to_string())?
        {
            let entry = entry.map_err(|_| "acceptance store entry is unreadable".to_string())?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) == Some("json") {
                paths.push(path);
            } else if !entry.file_name().to_string_lossy().starts_with(".tmp-") {
                return Err("acceptance store contains an unexpected entry".into());
            }
        }
        Ok(paths)
    }
}

pub fn build_packet(
    original_request: &str,
    revision: AcceptanceRevisionV1,
    input: &ReviewInput,
    work: &WorkState,
    round: &ReviewConvergenceRound,
    now: DateTime<Utc>,
) -> Result<AcceptancePacketV1, String> {
    if round.scope != input.scope {
        return Err("acceptance review scope does not match the captured source".into());
    }
    let mut paths = input
        .included_files
        .iter()
        .chain(input.untracked_files.iter())
        .cloned()
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    if paths.len() > MAX_PATHS || paths.iter().any(|path| !safe_relative_path(path)) {
        return Err("acceptance changed paths are invalid or exceed their limit".into());
    }
    let work_events = work
        .events
        .iter()
        .rev()
        .take(MAX_EVENTS)
        .rev()
        .map(|event| AcceptanceWorkEventV1 {
            id: event.id.0,
            kind: event.kind.clone(),
            succeeded: event.succeeded,
            summary_sha256: sha256(event.summary.as_bytes()),
            created_at: event.created_at,
        })
        .collect::<Vec<_>>();
    let receipt_digests = valid_agent_receipt_digests(work);
    let reviewers = round
        .reviewers
        .iter()
        .map(|reviewer| AcceptanceReviewerV1 {
            provider: reviewer.provider.clone(),
            model: reviewer.model.clone(),
            verdict: reviewer.verdict,
            finding_count: reviewer.findings.len(),
            summary_sha256: sha256(reviewer.summary.as_bytes()),
        })
        .collect::<Vec<_>>();
    let mut claims = work_events
        .iter()
        .map(|event| AcceptanceClaimV1 {
            label: if event.succeeded {
                AcceptanceClaimLabel::Verified
            } else {
                AcceptanceClaimLabel::Contradicted
            },
            subject: format!("host_work_event:{}", event.id),
            evidence_sha256: Some(event.summary_sha256.clone()),
        })
        .collect::<Vec<_>>();
    claims.extend(receipt_digests.iter().map(|digest| AcceptanceClaimV1 {
        label: AcceptanceClaimLabel::Verified,
        subject: "agent_verification_receipt".into(),
        evidence_sha256: Some(digest.clone()),
    }));
    claims.extend(round.reviewers.iter().map(|reviewer| AcceptanceClaimV1 {
        label: match reviewer.verdict {
            ReviewerVerdict::Pass => AcceptanceClaimLabel::Inferred,
            ReviewerVerdict::Findings => AcceptanceClaimLabel::Contradicted,
            ReviewerVerdict::Error => AcceptanceClaimLabel::Unverified,
        },
        subject: format!("reviewer:{}/{}", reviewer.provider, reviewer.model),
        evidence_sha256: Some(sha256(reviewer.summary.as_bytes())),
    }));
    if claims.len() > MAX_CLAIMS {
        return Err("acceptance claims exceed their limit".into());
    }
    let unresolved_risks = round
        .reviewers
        .iter()
        .flat_map(|reviewer| {
            reviewer.findings.iter().map(|finding| {
                bounded_text(&format!(
                    "{}/{}: {}",
                    reviewer.provider, reviewer.model, finding.title
                ))
            })
        })
        .take(MAX_RISKS)
        .collect::<Vec<_>>();
    let plan = work.plan.as_ref().map(|plan| AcceptancePlanBindingV1 {
        id: plan.id,
        revision: plan.revision,
    });
    let mut packet = AcceptancePacketV1 {
        schema: SCHEMA.into(),
        id: Uuid::new_v4(),
        created_at: now,
        request_sha256: sha256(original_request.as_bytes()),
        revision,
        plan,
        changed_relative_paths: paths,
        agent_receipt_sha256: receipt_digests,
        work_events,
        reviewers,
        reviewer_round: round.round,
        reviewer_requests: round.budget.provider_requests,
        reviewer_output_bytes: round.budget.output_bytes,
        known_cost_usd: round.budget.known_cost_usd.clone(),
        unpriced_requests: round.budget.unpriced_requests,
        unresolved_risks,
        claims,
        packet_sha256: String::new(),
    };
    validate_packet(&packet, false)?;
    packet.packet_sha256 = packet_sha256(&packet)?;
    Ok(packet)
}

pub fn capture_revision(
    input: &ReviewInput,
    workspace: &crate::workspace::WorkspaceContext,
    session_id: Option<Uuid>,
    messages: &[Message],
    work: &WorkState,
) -> Result<AcceptanceRevisionV1, String> {
    #[derive(Serialize)]
    struct Source<'a> {
        scope: &'a str,
        diff: &'a str,
        included_files: &'a [String],
        omitted_files: &'a [String],
        untracked_files: &'a [String],
        custom_instructions: &'a Option<String>,
    }
    #[derive(Serialize)]
    struct WorkspaceRoot<'a> {
        id: String,
        path_sha256: String,
        read_only: bool,
        primary: bool,
        #[serde(skip)]
        _root: std::marker::PhantomData<&'a ()>,
    }
    #[derive(Serialize)]
    struct Session<'a> {
        session_id: Option<Uuid>,
        messages: &'a [Message],
        work: &'a WorkState,
    }
    let source = serde_json::to_vec(&Source {
        scope: &input.scope,
        diff: &input.diff,
        included_files: &input.included_files,
        omitted_files: &input.omitted_files,
        untracked_files: &input.untracked_files,
        custom_instructions: &input.custom_instructions,
    })
    .map_err(|_| "acceptance source revision could not be encoded".to_string())?;
    let roots = workspace
        .roots()
        .iter()
        .map(|root| WorkspaceRoot {
            id: root.id.to_string(),
            path_sha256: sha256(root.path.as_os_str().as_encoded_bytes()),
            read_only: root.read_only,
            primary: root.primary,
            _root: std::marker::PhantomData,
        })
        .collect::<Vec<_>>();
    let workspace = serde_json::to_vec(&roots)
        .map_err(|_| "acceptance workspace revision could not be encoded".to_string())?;
    let session = serde_json::to_vec(&Session {
        session_id,
        messages,
        work,
    })
    .map_err(|_| "acceptance session revision could not be encoded".to_string())?;
    Ok(AcceptanceRevisionV1 {
        source_revision_sha256: sha256(&source),
        workspace_revision_sha256: sha256(&workspace),
        session_id,
        session_revision_sha256: sha256(&session),
    })
}

fn valid_agent_receipt_digests(work: &WorkState) -> Vec<String> {
    let binding = work.agent_graph.verification_binding();
    let Some(plan) = work.plan.as_ref() else {
        return Vec::new();
    };
    let mut receipts = plan
        .tasks
        .iter()
        .filter_map(|task| {
            let definition = task.agent.as_ref()?;
            let attempt = task.agent_runtime.attempts.last()?;
            let receipt = attempt.verification_receipt.as_ref()?;
            (receipt.disposition == crate::agent_graph::AgentVerificationDisposition::Pass
                && crate::agent_graph::verification_receipt_valid(
                    binding, task.id, definition, attempt,
                ))
            .then(|| receipt.receipt_sha256.clone())
        })
        .collect::<Vec<_>>();
    receipts.sort();
    receipts.dedup();
    receipts
}

fn validate_packet(packet: &AcceptancePacketV1, require_digest: bool) -> Result<(), String> {
    if packet.schema != SCHEMA
        || !is_sha256(&packet.request_sha256)
        || !is_sha256(&packet.revision.source_revision_sha256)
        || !is_sha256(&packet.revision.workspace_revision_sha256)
        || !is_sha256(&packet.revision.session_revision_sha256)
        || packet.changed_relative_paths.len() > MAX_PATHS
        || packet
            .changed_relative_paths
            .iter()
            .any(|path| !safe_relative_path(path))
        || packet.work_events.len() > MAX_EVENTS
        || packet.reviewers.len() != 2
        || packet.reviewer_round == 0
        || packet.reviewer_round > 3
        || packet.reviewer_requests > 48
        || packet.reviewer_output_bytes > 1024 * 1024
        || packet.unresolved_risks.len() > MAX_RISKS
        || packet.claims.len() > MAX_CLAIMS
        || packet
            .agent_receipt_sha256
            .iter()
            .chain(packet.work_events.iter().map(|event| &event.summary_sha256))
            .any(|digest| !is_sha256(digest))
        || packet
            .unresolved_risks
            .iter()
            .chain(packet.claims.iter().map(|claim| &claim.subject))
            .any(|value| value.len() > MAX_TEXT_BYTES || value.chars().any(char::is_control))
        || packet.claims.iter().any(|claim| {
            claim
                .evidence_sha256
                .as_deref()
                .is_some_and(|digest| !is_sha256(digest))
                || (claim.label == AcceptanceClaimLabel::Verified
                    && claim.evidence_sha256.is_none())
        })
    {
        return Err("acceptance packet failed bounded schema validation".into());
    }
    if require_digest
        && (!is_sha256(&packet.packet_sha256) || packet.packet_sha256 != packet_sha256(packet)?)
    {
        return Err("acceptance packet digest is invalid".into());
    }
    Ok(())
}

fn packet_sha256(packet: &AcceptancePacketV1) -> Result<String, String> {
    let mut binding = packet.clone();
    binding.packet_sha256.clear();
    serde_json::to_vec(&binding)
        .map(|bytes| sha256(&bytes))
        .map_err(|_| "acceptance packet digest could not be encoded".to_string())
}

fn safe_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && value.len() <= 4096
        && !path.is_absolute()
        && path.components().all(|component| {
            matches!(component, std::path::Component::Normal(_))
                && !component.as_os_str().is_empty()
        })
}

fn bounded_text(value: &str) -> String {
    let value = crate::terminal_text::sanitize_terminal_text(value);
    if value.len() <= MAX_TEXT_BYTES {
        return value;
    }
    let mut end = MAX_TEXT_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end.saturating_sub(3)])
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn ensure_private_dir(path: &Path) -> Result<(), String> {
    if path.exists() {
        return validate_private_dir(path);
    }
    std::fs::create_dir_all(path)
        .map_err(|_| "acceptance store directory could not be created".to_string())?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| "acceptance store permissions could not be set".to_string())?;
    validate_private_dir(path)
}

fn validate_private_dir(path: &Path) -> Result<(), String> {
    let link = std::fs::symlink_metadata(path)
        .map_err(|_| "acceptance store metadata is unavailable".to_string())?;
    if link.file_type().is_symlink() || !link.is_dir() {
        return Err("acceptance store must be a private regular directory".into());
    }
    let metadata = std::fs::metadata(path)
        .map_err(|_| "acceptance store metadata is unavailable".to_string())?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.permissions().mode() & 0o077 != 0 {
        return Err("acceptance store ownership or permissions are unsafe".into());
    }
    Ok(())
}

fn private_packet_metadata(path: &Path) -> Result<std::fs::Metadata, String> {
    let link = std::fs::symlink_metadata(path)
        .map_err(|_| "acceptance packet metadata is unavailable".to_string())?;
    if link.file_type().is_symlink() || !link.is_file() {
        return Err("acceptance packet must be a regular file".into());
    }
    let metadata = std::fs::metadata(path)
        .map_err(|_| "acceptance packet metadata is unavailable".to_string())?;
    if metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.nlink() != 1
    {
        return Err("acceptance packet ownership, mode, or link count is unsafe".into());
    }
    Ok(metadata)
}

fn publish_private_immutable(
    parent: &Path,
    destination: &Path,
    bytes: &[u8],
) -> Result<(), String> {
    let temporary = parent.join(format!(".tmp-{}", Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW);
    let mut file = options
        .open(&temporary)
        .map_err(|_| "acceptance staging file could not be reserved".to_string())?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|_| "acceptance staging file could not be written".to_string())?;
        file.sync_all()
            .map_err(|_| "acceptance staging file could not be synchronized".to_string())?;
        drop(file);
        std::fs::hard_link(&temporary, destination).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                "acceptance destination already exists".to_string()
            } else {
                "acceptance packet could not be published atomically".to_string()
            }
        })?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| "acceptance directory could not be synchronized".to_string())?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&temporary);
    if result.is_err() {
        let _ = std::fs::remove_file(destination);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{ReviewBudgetSnapshot, ReviewResult};
    use crate::review_convergence::{ReviewerOutcome, ReviewerVerdict};
    use std::os::unix::fs::PermissionsExt;

    fn input() -> ReviewInput {
        ReviewInput {
            scope: "working tree".into(),
            diff: "diff --git a/src/lib.rs b/src/lib.rs\n+change".into(),
            included_files: vec!["src/lib.rs".into()],
            omitted_files: Vec::new(),
            untracked_files: vec!["notes.txt".into()],
            custom_instructions: None,
        }
    }

    fn round(passed: bool) -> ReviewConvergenceRound {
        let reviewer = |provider: &str, verdict| ReviewerOutcome {
            provider: provider.into(),
            model: "reviewer".into(),
            verdict,
            summary: "SECRET REVIEW BODY".into(),
            findings: if verdict == ReviewerVerdict::Pass {
                Vec::new()
            } else {
                ReviewResult {
                    findings: Vec::new(),
                    summary: String::new(),
                }
                .findings
            },
        };
        ReviewConvergenceRound {
            round: 1,
            scope: "working tree".into(),
            reviewers: [
                reviewer("first", ReviewerVerdict::Pass),
                reviewer(
                    "second",
                    if passed {
                        ReviewerVerdict::Pass
                    } else {
                        ReviewerVerdict::Error
                    },
                ),
            ],
            budget: ReviewBudgetSnapshot {
                provider_requests: 2,
                output_bytes: 100,
                known_cost_usd: "0.01".into(),
                unpriced_requests: 0,
            },
            passed,
        }
    }

    fn packet(root: &Path, passed: bool) -> AcceptancePacketV1 {
        let workspace = crate::workspace::WorkspaceContext::discover(root).unwrap();
        let work = WorkState::default();
        let revision = capture_revision(&input(), &workspace, None, &[], &work).unwrap();
        build_packet(
            "SECRET ORIGINAL REQUEST",
            revision,
            &input(),
            &work,
            &round(passed),
            Utc::now(),
        )
        .unwrap()
    }

    #[test]
    fn packet_retains_only_digests_and_host_evidence_controls_verified_labels() {
        let root = tempfile::tempdir().unwrap();
        let mut work = WorkState::default();
        work.record_event(WorkEventKind::Test, "SECRET TOOL OUTPUT", true);
        let workspace = crate::workspace::WorkspaceContext::discover(root.path()).unwrap();
        let revision = capture_revision(&input(), &workspace, None, &[], &work).unwrap();
        let packet = build_packet(
            "SECRET ORIGINAL REQUEST",
            revision,
            &input(),
            &work,
            &round(true),
            Utc::now(),
        )
        .unwrap();
        let encoded = serde_json::to_string(&packet).unwrap();
        assert!(!encoded.contains("SECRET ORIGINAL REQUEST"));
        assert!(!encoded.contains("SECRET REVIEW BODY"));
        assert!(!encoded.contains("SECRET TOOL OUTPUT"));
        assert!(packet.claims.iter().any(|claim| {
            claim.label == AcceptanceClaimLabel::Verified
                && claim.subject.starts_with("host_work_event:")
        }));
        assert!(packet
            .claims
            .iter()
            .filter(|claim| { claim.subject.starts_with("reviewer:") })
            .all(|claim| claim.label == AcceptanceClaimLabel::Inferred));
        assert!(validate_packet(&packet, true).is_ok());
    }

    #[test]
    fn private_store_is_immutable_digest_checked_and_exports_without_overwrite() {
        let root = tempfile::tempdir().unwrap();
        let store_root = root.path().join("acceptance");
        let store = AcceptanceStore::new(&store_root);
        let path = store.create(packet(root.path(), true)).unwrap();
        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(store.list(None).unwrap().len(), 1);
        assert!(store.latest().unwrap().is_some());

        let export = root.path().join("export.json");
        store.export_latest(&export).unwrap();
        assert_eq!(
            std::fs::metadata(&export).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(store
            .export_latest(&export)
            .unwrap_err()
            .contains("overwrite"));

        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["known_cost_usd"] = serde_json::Value::String("999".into());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(store.latest().unwrap_err().contains("digest"));
    }

    #[test]
    fn revision_changes_make_summaries_stale_without_mutating_packets() {
        let root = tempfile::tempdir().unwrap();
        let store = AcceptanceStore::new(root.path().join("acceptance"));
        let packet = packet(root.path(), true);
        let current = packet.revision.clone();
        store.create(packet).unwrap();
        assert!(!store.list(Some(&current)).unwrap()[0].stale);
        let mut changed = current;
        changed.session_revision_sha256 = "f".repeat(64);
        assert!(store.list(Some(&changed)).unwrap()[0].stale);
    }

    #[cfg(unix)]
    #[test]
    fn store_refuses_symlink_and_linked_packet_hazards() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        std::fs::create_dir(&real).unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700)).unwrap();
        let linked = root.path().join("linked");
        symlink(&real, &linked).unwrap();
        assert!(AcceptanceStore::new(&linked)
            .create(packet(root.path(), true))
            .unwrap_err()
            .contains("directory"));

        let store = AcceptanceStore::new(root.path().join("packets"));
        let path = store.create(packet(root.path(), true)).unwrap();
        std::fs::hard_link(&path, root.path().join("other.json")).unwrap();
        assert!(store.latest().unwrap_err().contains("link count"));
    }
}
