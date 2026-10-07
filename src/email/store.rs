use super::types::{
    sha256_hex, AccountId, AttachmentPreview, EmailAddress, EmailDeliveryReceipt,
    EmailDeliveryState, EmailDraftV1, ExternalCommunicationPreview, RecipientSet, StagedAttachment,
    ThreadingFields, MAX_ATTACHMENTS, MAX_ATTACHMENT_BYTES, MAX_BODY_BYTES,
    MAX_TOTAL_ATTACHMENT_BYTES,
};
use base64::Engine;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

const MAX_DRAFT_RECORD_BYTES: u64 = 512 * 1024;
const MAX_GATEWAY_JOURNAL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_GATEWAY_EVENT_BYTES: usize = 32 * 1024;
const MAX_GATEWAY_PAYLOAD_BYTES: u64 = 512 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum EmailStoreError {
    #[error("email store is unavailable")]
    Io,
    #[error("email draft is invalid")]
    InvalidDraft,
    #[error("email draft is missing")]
    MissingDraft,
    #[error("email draft integrity check failed")]
    DraftDrift,
    #[error("email attachment is unsafe")]
    UnsafeAttachment,
    #[error("email attachment integrity check failed")]
    AttachmentDrift,
    #[error("email store record already exists")]
    AlreadyExists,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct GatewayMessageIdentity {
    pub account_id: String,
    pub folder: String,
    pub uid_validity: u32,
    pub uid: u32,
}

impl GatewayMessageIdentity {
    pub fn storage_key(&self) -> String {
        sha256_hex(
            format!(
                "{}\0{}\0{}\0{}",
                self.account_id, self.folder, self.uid_validity, self.uid
            )
            .as_bytes(),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum GatewayEvent {
    Bootstrap {
        uid_validity: u32,
        high_water: u32,
    },
    Resynchronized {
        uid_validity: u32,
        high_water: u32,
    },
    Observed {
        identity: GatewayMessageIdentity,
        content_sha256: String,
        message_id_sha256: Option<String>,
    },
    Ignored {
        identity: GatewayMessageIdentity,
        reason: String,
    },
    Authorized {
        identity: GatewayMessageIdentity,
    },
    Processing {
        identity: GatewayMessageIdentity,
        request_sha256: String,
    },
    ResponseReady {
        identity: GatewayMessageIdentity,
        draft_sha256: String,
        deterministic_message_id: String,
    },
    DraftAppendStarted {
        identity: GatewayMessageIdentity,
    },
    Drafted {
        identity: GatewayMessageIdentity,
        drafts_uid: u32,
    },
    Ambiguous {
        identity: GatewayMessageIdentity,
    },
    Failed {
        identity: GatewayMessageIdentity,
        code: String,
    },
}

impl GatewayEvent {
    pub fn identity(&self) -> Option<&GatewayMessageIdentity> {
        match self {
            Self::Bootstrap { .. } | Self::Resynchronized { .. } => None,
            Self::Observed { identity, .. }
            | Self::Ignored { identity, .. }
            | Self::Authorized { identity }
            | Self::Processing { identity, .. }
            | Self::ResponseReady { identity, .. }
            | Self::DraftAppendStarted { identity }
            | Self::Drafted { identity, .. }
            | Self::Ambiguous { identity }
            | Self::Failed { identity, .. } => Some(identity),
        }
    }

    pub fn terminal(&self) -> bool {
        matches!(
            self,
            Self::Ignored { .. }
                | Self::Drafted { .. }
                | Self::Ambiguous { .. }
                | Self::Failed { .. }
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct GatewayJournalRecord {
    schema: String,
    sequence: u64,
    previous_sha256: String,
    timestamp: String,
    event: GatewayEvent,
    record_sha256: String,
}

#[derive(Debug, Clone, Default)]
pub struct GatewaySnapshot {
    pub uid_validity: Option<u32>,
    pub high_water: u32,
    pub events: Vec<GatewayEvent>,
}

impl GatewaySnapshot {
    pub fn terminal_for(&self, identity: &GatewayMessageIdentity) -> Option<&GatewayEvent> {
        self.events
            .iter()
            .rev()
            .find(|event| event.identity() == Some(identity) && event.terminal())
    }

    pub fn has_content_digest(&self, digest: &str) -> bool {
        self.events.iter().any(|event| matches!(event, GatewayEvent::Observed { content_sha256, .. } if content_sha256 == digest))
    }

    pub fn has_message_id_digest(&self, digest: &str) -> bool {
        self.events.iter().any(|event| matches!(event, GatewayEvent::Observed { message_id_sha256: Some(value), .. } if value == digest))
    }

    pub fn pending_responses(&self) -> Vec<(GatewayMessageIdentity, String, String)> {
        let mut output = Vec::new();
        for event in &self.events {
            let GatewayEvent::ResponseReady {
                identity,
                draft_sha256,
                deterministic_message_id,
            } = event
            else {
                continue;
            };
            if self.terminal_for(identity).is_none() {
                output.push((
                    identity.clone(),
                    draft_sha256.clone(),
                    deterministic_message_id.clone(),
                ));
            }
        }
        output
    }

    pub fn incomplete_without_response(&self) -> Vec<GatewayMessageIdentity> {
        let mut identities = std::collections::BTreeSet::new();
        let mut responses = std::collections::BTreeSet::new();
        for event in &self.events {
            if let Some(identity) = event.identity() {
                if event.terminal() {
                    identities.remove(identity);
                    responses.remove(identity);
                } else {
                    identities.insert(identity.clone());
                }
            }
            if let GatewayEvent::ResponseReady { identity, .. } = event {
                responses.insert(identity.clone());
            }
        }
        identities
            .into_iter()
            .filter(|identity| !responses.contains(identity))
            .collect()
    }
}

#[derive(Clone)]
pub struct GatewayStore {
    root: PathBuf,
}

impl std::fmt::Debug for GatewayStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GatewayStore")
            .field("root", &"[PRIVATE GATEWAY STORE]")
            .finish()
    }
}

pub struct GatewayLock {
    #[cfg(unix)]
    file: File,
    #[cfg(not(unix))]
    path: PathBuf,
}

impl Drop for GatewayLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            use std::os::fd::AsRawFd;
            let _ = libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
        #[cfg(not(unix))]
        let _ = fs::remove_file(&self.path);
    }
}

impl GatewayStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn lock(&self) -> Result<GatewayLock, EmailStoreError> {
        self.ensure_gateway_layout()?;
        let path = self.root.join("gateway.lock");
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            if path.exists() {
                let metadata = fs::symlink_metadata(&path).map_err(|_| EmailStoreError::Io)?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(EmailStoreError::Io);
                }
            }
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(&path)
                .map_err(|_| EmailStoreError::Io)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .map_err(|_| EmailStoreError::Io)?;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(EmailStoreError::AlreadyExists);
            }
            Ok(GatewayLock { file })
        }
        #[cfg(not(unix))]
        {
            write_create_new(&path, std::process::id().to_string().as_bytes())?;
            Ok(GatewayLock { path })
        }
    }

    pub fn load(&self) -> Result<GatewaySnapshot, EmailStoreError> {
        self.ensure_gateway_layout()?;
        let path = self.journal_path();
        if !path.exists() {
            return Ok(GatewaySnapshot::default());
        }
        let bytes = read_bounded(&path, MAX_GATEWAY_JOURNAL_BYTES)?;
        let mut snapshot = GatewaySnapshot::default();
        let mut previous = sha256_hex(b"ca-email-gateway-journal-v1");
        for (index, line) in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .enumerate()
        {
            if line.len() > MAX_GATEWAY_EVENT_BYTES {
                return Err(EmailStoreError::DraftDrift);
            }
            let record: GatewayJournalRecord =
                serde_json::from_slice(line).map_err(|_| EmailStoreError::DraftDrift)?;
            let expected = gateway_record_digest(
                record.sequence,
                &record.previous_sha256,
                &record.timestamp,
                &record.event,
            )?;
            if record.schema != "ca.email-gateway-event.v1"
                || record.sequence != index as u64 + 1
                || record.previous_sha256 != previous
                || record.record_sha256 != expected
            {
                return Err(EmailStoreError::DraftDrift);
            }
            previous = record.record_sha256;
            match &record.event {
                GatewayEvent::Bootstrap {
                    uid_validity,
                    high_water,
                }
                | GatewayEvent::Resynchronized {
                    uid_validity,
                    high_water,
                } => {
                    snapshot.uid_validity = Some(*uid_validity);
                    snapshot.high_water = *high_water;
                }
                event => {
                    if let Some(identity) = event.identity() {
                        snapshot.high_water = snapshot.high_water.max(identity.uid);
                    }
                }
            }
            snapshot.events.push(record.event);
        }
        Ok(snapshot)
    }

    pub fn append(&self, event: GatewayEvent) -> Result<(), EmailStoreError> {
        let snapshot = self.load()?;
        let (sequence, previous) = if let Some(last) = self.last_record()? {
            (last.sequence + 1, last.record_sha256)
        } else {
            (1, sha256_hex(b"ca-email-gateway-journal-v1"))
        };
        if sequence != snapshot.events.len() as u64 + 1 {
            return Err(EmailStoreError::DraftDrift);
        }
        let timestamp = Utc::now().to_rfc3339();
        let digest = gateway_record_digest(sequence, &previous, &timestamp, &event)?;
        let record = GatewayJournalRecord {
            schema: "ca.email-gateway-event.v1".into(),
            sequence,
            previous_sha256: previous,
            timestamp,
            event,
            record_sha256: digest,
        };
        let mut bytes = serde_json::to_vec(&record).map_err(|_| EmailStoreError::Io)?;
        if bytes.len() > MAX_GATEWAY_EVENT_BYTES {
            return Err(EmailStoreError::InvalidDraft);
        }
        bytes.push(b'\n');
        let path = self.journal_path();
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(|_| EmailStoreError::Io)?;
        file.write_all(&bytes).map_err(|_| EmailStoreError::Io)?;
        file.sync_all().map_err(|_| EmailStoreError::Io)?;
        sync_dir(&self.root)
    }

    pub fn store_payload(
        &self,
        identity: &GatewayMessageIdentity,
        bytes: &[u8],
    ) -> Result<String, EmailStoreError> {
        if bytes.is_empty() || bytes.len() as u64 > MAX_GATEWAY_PAYLOAD_BYTES {
            return Err(EmailStoreError::InvalidDraft);
        }
        self.ensure_gateway_layout()?;
        let digest = sha256_hex(bytes);
        let path = self.payload_path(identity);
        write_create_new(&path, bytes)?;
        Ok(digest)
    }

    pub fn load_payload(
        &self,
        identity: &GatewayMessageIdentity,
        expected_sha256: &str,
    ) -> Result<Vec<u8>, EmailStoreError> {
        let bytes = read_bounded(&self.payload_path(identity), MAX_GATEWAY_PAYLOAD_BYTES)?;
        if sha256_hex(&bytes) != expected_sha256 {
            return Err(EmailStoreError::DraftDrift);
        }
        Ok(bytes)
    }

    fn ensure_gateway_layout(&self) -> Result<(), EmailStoreError> {
        ensure_owner_path(&self.root)?;
        ensure_owner_path(&self.root.join("payloads"))
    }
    fn journal_path(&self) -> PathBuf {
        self.root.join("events.jsonl")
    }
    fn payload_path(&self, identity: &GatewayMessageIdentity) -> PathBuf {
        self.root
            .join("payloads")
            .join(format!("{}.eml", identity.storage_key()))
    }
    fn last_record(&self) -> Result<Option<GatewayJournalRecord>, EmailStoreError> {
        let path = self.journal_path();
        if !path.exists() {
            return Ok(None);
        }
        let bytes = read_bounded(&path, MAX_GATEWAY_JOURNAL_BYTES)?;
        let line = bytes
            .split(|byte| *byte == b'\n')
            .rfind(|line| !line.is_empty());
        line.map(|line| serde_json::from_slice(line).map_err(|_| EmailStoreError::DraftDrift))
            .transpose()
    }
}

fn gateway_record_digest(
    sequence: u64,
    previous: &str,
    timestamp: &str,
    event: &GatewayEvent,
) -> Result<String, EmailStoreError> {
    let bytes = serde_json::to_vec(&(sequence, previous, timestamp, event))
        .map_err(|_| EmailStoreError::Io)?;
    Ok(sha256_hex(&bytes))
}

#[derive(Clone)]
pub struct EmailStore {
    root: PathBuf,
}

impl std::fmt::Debug for EmailStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EmailStore")
            .field("root", &"[PRIVATE EMAIL STORE]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct DraftReceipt {
    schema: String,
    draft_ref: String,
    draft_sha256: String,
    draft_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct DeliveryEvent {
    schema: String,
    receipt: EmailDeliveryReceipt,
    draft_sha256: String,
    phase: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDraft {
    pub draft_ref: String,
    pub draft_sha256: String,
    pub draft_bytes: u64,
}

impl EmailStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_draft(
        &self,
        account_id: AccountId,
        from: EmailAddress,
        recipients: RecipientSet,
        subject: String,
        body: String,
        threading: Option<ThreadingFields>,
        attachment_paths: &[PathBuf],
    ) -> Result<StoredDraft, EmailStoreError> {
        if body.len() > MAX_BODY_BYTES || attachment_paths.len() > MAX_ATTACHMENTS {
            return Err(EmailStoreError::InvalidDraft);
        }
        self.ensure_layout()?;
        let draft_ref = format!("draft_{}", Uuid::new_v4().simple());
        let attachment_dir = self.attachments_dir().join(&draft_ref);
        create_owner_dir(&attachment_dir)?;
        let attachments = match self.stage_attachments(&attachment_dir, attachment_paths) {
            Ok(attachments) => attachments,
            Err(error) => {
                let _ = fs::remove_dir_all(&attachment_dir);
                return Err(error);
            }
        };
        let draft = EmailDraftV1::new(
            account_id,
            from,
            recipients,
            subject,
            body,
            threading,
            attachments,
        )
        .map_err(|_| EmailStoreError::InvalidDraft)?;
        let bytes = serde_json::to_vec(&draft).map_err(|_| EmailStoreError::InvalidDraft)?;
        if bytes.len() as u64 > MAX_DRAFT_RECORD_BYTES {
            return Err(EmailStoreError::InvalidDraft);
        }
        let digest = sha256_hex(&bytes);
        let draft_path = self.draft_path(&draft_ref)?;
        write_create_new(&draft_path, &bytes)?;
        let receipt = DraftReceipt {
            schema: "ca.email-draft-receipt.v1".into(),
            draft_ref: draft_ref.clone(),
            draft_sha256: digest.clone(),
            draft_bytes: bytes.len() as u64,
        };
        let receipt_bytes = serde_json::to_vec(&receipt).map_err(|_| EmailStoreError::Io)?;
        if let Err(error) = write_create_new(&self.receipt_path(&draft_ref)?, &receipt_bytes) {
            let _ = fs::remove_file(&draft_path);
            let _ = fs::remove_dir_all(&attachment_dir);
            return Err(error);
        }
        sync_dir(&self.drafts_dir())?;
        Ok(StoredDraft {
            draft_ref,
            draft_sha256: digest,
            draft_bytes: bytes.len() as u64,
        })
    }

    pub fn load_draft(&self, draft_ref: &str) -> Result<EmailDraftV1, EmailStoreError> {
        let receipt_path = self.receipt_path(draft_ref)?;
        let receipt_bytes = read_bounded(&receipt_path, 16 * 1024)?;
        let receipt: DraftReceipt =
            serde_json::from_slice(&receipt_bytes).map_err(|_| EmailStoreError::DraftDrift)?;
        if receipt.schema != "ca.email-draft-receipt.v1" || receipt.draft_ref != draft_ref {
            return Err(EmailStoreError::DraftDrift);
        }
        let bytes = read_bounded(&self.draft_path(draft_ref)?, MAX_DRAFT_RECORD_BYTES)?;
        if bytes.len() as u64 != receipt.draft_bytes || sha256_hex(&bytes) != receipt.draft_sha256 {
            return Err(EmailStoreError::DraftDrift);
        }
        let draft: EmailDraftV1 =
            serde_json::from_slice(&bytes).map_err(|_| EmailStoreError::DraftDrift)?;
        if draft.schema != "ca.email-draft.v1" || draft.digest() != receipt.draft_sha256 {
            return Err(EmailStoreError::DraftDrift);
        }
        self.verify_attachments(draft_ref, &draft.attachments)?;
        Ok(draft)
    }

    pub fn preview(
        &self,
        draft_ref: &str,
    ) -> Result<ExternalCommunicationPreview, EmailStoreError> {
        let draft = self.load_draft(draft_ref)?;
        Ok(ExternalCommunicationPreview {
            account_id: draft.account_id.as_str().into(),
            to: draft
                .recipients
                .to
                .iter()
                .map(|address| address.as_str().into())
                .collect(),
            cc: draft
                .recipients
                .cc
                .iter()
                .map(|address| address.as_str().into())
                .collect(),
            bcc: draft
                .recipients
                .bcc
                .iter()
                .map(|address| address.as_str().into())
                .collect(),
            subject: draft.subject.clone(),
            body_bytes: draft.body.len(),
            body_sha256: sha256_hex(draft.body.as_bytes()),
            draft_sha256: draft.digest(),
            attachments: draft
                .attachments
                .iter()
                .map(|attachment| AttachmentPreview {
                    basename: attachment.basename.clone(),
                    size: attachment.size,
                    sha256: attachment.sha256.clone(),
                })
                .collect(),
        })
    }

    pub fn render_message(&self, draft_ref: &str) -> Result<Vec<u8>, EmailStoreError> {
        let draft = self.load_draft(draft_ref)?;
        let mut output = Vec::new();
        push_header(&mut output, "From", draft.from.as_str())?;
        if !draft.recipients.to.is_empty() {
            push_header(&mut output, "To", &join_addresses(&draft.recipients.to))?;
        }
        if !draft.recipients.cc.is_empty() {
            push_header(&mut output, "Cc", &join_addresses(&draft.recipients.cc))?;
        }
        // Bcc is intentionally envelope-only. The adapter passes the complete
        // recipient set as explicit SMTP RCPT TO arguments.
        push_header(&mut output, "Subject", &draft.subject)?;
        if let Some(threading) = &draft.threading {
            push_header(
                &mut output,
                "In-Reply-To",
                &format!("<{}>", threading.in_reply_to.as_str()),
            )?;
            if !threading.references.is_empty() {
                let references = threading
                    .references
                    .iter()
                    .map(|id| format!("<{}>", id.as_str()))
                    .collect::<Vec<_>>()
                    .join(" ");
                push_header(&mut output, "References", &references)?;
            }
        }
        let digest = draft.digest();
        push_header(
            &mut output,
            "Message-ID",
            &format!("<ca-{}@localhost>", &digest[..32]),
        )?;
        push_header(&mut output, "MIME-Version", "1.0")?;
        if draft.attachments.is_empty() {
            push_header(&mut output, "Content-Type", "text/plain; charset=utf-8")?;
            output.extend_from_slice(b"Content-Transfer-Encoding: 8bit\r\n\r\n");
            push_crlf_body(&mut output, &draft.body);
            return Ok(output);
        }
        let boundary = format!("ca-{}", &digest[..40]);
        push_header(
            &mut output,
            "Content-Type",
            &format!("multipart/mixed; boundary=\"{boundary}\""),
        )?;
        output.extend_from_slice(b"\r\n");
        output.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        output.extend_from_slice(
            b"Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n",
        );
        push_crlf_body(&mut output, &draft.body);
        output.extend_from_slice(b"\r\n");
        for attachment in &draft.attachments {
            let bytes = read_bounded(
                &self
                    .attachments_dir()
                    .join(draft_ref)
                    .join(&attachment.stored_name),
                MAX_ATTACHMENT_BYTES,
            )?;
            output.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            output.extend_from_slice(b"Content-Type: application/octet-stream\r\n");
            output.extend_from_slice(
                format!(
                    "Content-Disposition: attachment; filename=\"{}\"\r\n",
                    attachment.basename
                )
                .as_bytes(),
            );
            output.extend_from_slice(b"Content-Transfer-Encoding: base64\r\n\r\n");
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            for chunk in encoded.as_bytes().chunks(76) {
                output.extend_from_slice(chunk);
                output.extend_from_slice(b"\r\n");
            }
        }
        output.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        Ok(output)
    }

    pub fn record_delivery(
        &self,
        draft_sha256: &str,
        state: EmailDeliveryState,
        account_id: &str,
        recipient_count: usize,
        phase: &str,
    ) -> Result<EmailDeliveryReceipt, EmailStoreError> {
        self.ensure_layout()?;
        let receipt = EmailDeliveryReceipt {
            receipt_id: format!("receipt_{}", Uuid::new_v4().simple()),
            state,
            account_digest: sha256_hex(account_id.as_bytes()),
            recipient_count,
            timestamp: Utc::now().to_rfc3339(),
            message_id_digest: (state == EmailDeliveryState::Delivered)
                .then(|| sha256_hex(format!("ca-{draft_sha256}").as_bytes())),
        };
        let event = DeliveryEvent {
            schema: "ca.email-delivery-event.v1".into(),
            receipt: receipt.clone(),
            draft_sha256: draft_sha256.into(),
            phase: phase.into(),
        };
        let bytes = serde_json::to_vec(&event).map_err(|_| EmailStoreError::Io)?;
        write_create_new(
            &self
                .delivery_dir()
                .join(format!("{}.json", receipt.receipt_id)),
            &bytes,
        )?;
        sync_dir(&self.delivery_dir())?;
        Ok(receipt)
    }

    fn ensure_layout(&self) -> Result<(), EmailStoreError> {
        create_owner_dir(&self.root)?;
        create_owner_dir(&self.drafts_dir())?;
        create_owner_dir(&self.attachments_dir())?;
        create_owner_dir(&self.delivery_dir())
    }

    fn drafts_dir(&self) -> PathBuf {
        self.root.join("drafts")
    }

    fn attachments_dir(&self) -> PathBuf {
        self.root.join("attachments")
    }

    fn delivery_dir(&self) -> PathBuf {
        self.root.join("delivery")
    }

    fn draft_path(&self, draft_ref: &str) -> Result<PathBuf, EmailStoreError> {
        validate_ref(draft_ref)?;
        Ok(self.drafts_dir().join(format!("{draft_ref}.json")))
    }

    fn receipt_path(&self, draft_ref: &str) -> Result<PathBuf, EmailStoreError> {
        validate_ref(draft_ref)?;
        Ok(self.drafts_dir().join(format!("{draft_ref}.receipt.json")))
    }

    fn stage_attachments(
        &self,
        directory: &Path,
        paths: &[PathBuf],
    ) -> Result<Vec<StagedAttachment>, EmailStoreError> {
        let mut staged = Vec::with_capacity(paths.len());
        let mut total = 0_u64;
        for (index, path) in paths.iter().enumerate() {
            if path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
            {
                return Err(EmailStoreError::UnsafeAttachment);
            }
            let metadata =
                fs::symlink_metadata(path).map_err(|_| EmailStoreError::UnsafeAttachment)?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(EmailStoreError::UnsafeAttachment);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.nlink() != 1 {
                    return Err(EmailStoreError::UnsafeAttachment);
                }
            }
            let size = metadata.len();
            total = total
                .checked_add(size)
                .ok_or(EmailStoreError::UnsafeAttachment)?;
            if size > MAX_ATTACHMENT_BYTES || total > MAX_TOTAL_ATTACHMENT_BYTES {
                return Err(EmailStoreError::UnsafeAttachment);
            }
            let basename = path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| safe_basename(name))
                .ok_or(EmailStoreError::UnsafeAttachment)?
                .to_string();
            let stored_name = format!("attachment-{index:02}.bin");
            let target = directory.join(&stored_name);
            let mut source = File::open(path).map_err(|_| EmailStoreError::UnsafeAttachment)?;
            let opened = source
                .metadata()
                .map_err(|_| EmailStoreError::UnsafeAttachment)?;
            if opened.len() != size {
                return Err(EmailStoreError::AttachmentDrift);
            }
            let mut bytes = Vec::with_capacity(size as usize);
            std::io::Read::by_ref(&mut source)
                .take(MAX_ATTACHMENT_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| EmailStoreError::UnsafeAttachment)?;
            if bytes.len() as u64 != size {
                return Err(EmailStoreError::AttachmentDrift);
            }
            write_create_new(&target, &bytes)?;
            staged.push(StagedAttachment {
                basename,
                size,
                sha256: sha256_hex(&bytes),
                stored_name,
            });
        }
        sync_dir(directory)?;
        Ok(staged)
    }

    fn verify_attachments(
        &self,
        draft_ref: &str,
        attachments: &[StagedAttachment],
    ) -> Result<(), EmailStoreError> {
        validate_ref(draft_ref)?;
        if attachments.len() > MAX_ATTACHMENTS {
            return Err(EmailStoreError::AttachmentDrift);
        }
        let directory = self.attachments_dir().join(draft_ref);
        for attachment in attachments {
            if !safe_basename(&attachment.stored_name) {
                return Err(EmailStoreError::AttachmentDrift);
            }
            let path = directory.join(&attachment.stored_name);
            let metadata =
                fs::symlink_metadata(&path).map_err(|_| EmailStoreError::AttachmentDrift)?;
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.len() != attachment.size
            {
                return Err(EmailStoreError::AttachmentDrift);
            }
            let bytes = read_bounded(&path, MAX_ATTACHMENT_BYTES)?;
            if sha256_hex(&bytes) != attachment.sha256 {
                return Err(EmailStoreError::AttachmentDrift);
            }
        }
        Ok(())
    }
}

fn validate_ref(draft_ref: &str) -> Result<(), EmailStoreError> {
    if draft_ref.len() != 38
        || !draft_ref.starts_with("draft_")
        || !draft_ref[6..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(EmailStoreError::InvalidDraft);
    }
    Ok(())
}

fn safe_basename(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value != "."
        && value != ".."
        && !value.contains(['/', '\\', '"', '\r', '\n', '\0'])
        && !value.chars().any(char::is_control)
}

fn join_addresses(addresses: &[EmailAddress]) -> String {
    addresses
        .iter()
        .map(EmailAddress::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

fn push_header(output: &mut Vec<u8>, name: &str, value: &str) -> Result<(), EmailStoreError> {
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-')
        || value.contains(['\r', '\n', '\0'])
        || value.len() > 8 * 1024
    {
        return Err(EmailStoreError::InvalidDraft);
    }
    output.extend_from_slice(name.as_bytes());
    output.extend_from_slice(b": ");
    output.extend_from_slice(value.as_bytes());
    output.extend_from_slice(b"\r\n");
    Ok(())
}

fn push_crlf_body(output: &mut Vec<u8>, body: &str) {
    let normalized = body.replace("\r\n", "\n").replace('\r', "\n");
    for (index, line) in normalized.split('\n').enumerate() {
        if index > 0 {
            output.extend_from_slice(b"\r\n");
        }
        output.extend_from_slice(line.as_bytes());
    }
    output.extend_from_slice(b"\r\n");
}

fn create_owner_dir(path: &Path) -> Result<(), EmailStoreError> {
    fs::create_dir_all(path).map_err(|_| EmailStoreError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| EmailStoreError::Io)?;
    }
    Ok(())
}

fn ensure_owner_dir_strict(path: &Path) -> Result<(), EmailStoreError> {
    if path.exists() {
        let metadata = fs::symlink_metadata(path).map_err(|_| EmailStoreError::Io)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(EmailStoreError::Io);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.uid() != unsafe { libc::geteuid() } {
                return Err(EmailStoreError::Io);
            }
        }
    } else {
        fs::create_dir(path).map_err(|_| EmailStoreError::Io)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| EmailStoreError::Io)?;
    }
    Ok(())
}

fn ensure_owner_path(path: &Path) -> Result<(), EmailStoreError> {
    let mut missing = Vec::new();
    let mut cursor = path;
    while !cursor.exists() {
        missing.push(cursor.to_path_buf());
        cursor = cursor.parent().ok_or(EmailStoreError::Io)?;
    }
    let ancestor = fs::symlink_metadata(cursor).map_err(|_| EmailStoreError::Io)?;
    if ancestor.file_type().is_symlink() || !ancestor.is_dir() {
        return Err(EmailStoreError::Io);
    }
    for directory in missing.into_iter().rev() {
        fs::create_dir(&directory).map_err(|_| EmailStoreError::Io)?;
        ensure_owner_dir_strict(&directory)?;
    }
    ensure_owner_dir_strict(path)
}

fn write_create_new(path: &Path, bytes: &[u8]) -> Result<(), EmailStoreError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            EmailStoreError::AlreadyExists
        } else {
            EmailStoreError::Io
        }
    })?;
    file.write_all(bytes).map_err(|_| EmailStoreError::Io)?;
    file.sync_all().map_err(|_| EmailStoreError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o400))
            .map_err(|_| EmailStoreError::Io)?;
    }
    Ok(())
}

fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, EmailStoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            EmailStoreError::MissingDraft
        } else {
            EmailStoreError::Io
        }
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > max {
        return Err(EmailStoreError::DraftDrift);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .map_err(|_| EmailStoreError::Io)?
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| EmailStoreError::Io)?;
    if bytes.len() as u64 > max {
        return Err(EmailStoreError::DraftDrift);
    }
    Ok(bytes)
}

fn sync_dir(path: &Path) -> Result<(), EmailStoreError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| EmailStoreError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email::types::{EmailAddress, RecipientSet};

    fn base(root: &Path) -> EmailStore {
        EmailStore::new(root.join("email"))
    }

    fn draft(store: &EmailStore, attachments: &[PathBuf]) -> StoredDraft {
        store
            .create_draft(
                AccountId::parse("personal").unwrap(),
                EmailAddress::parse("me@example.com").unwrap(),
                RecipientSet::parse(
                    &["to@example.com".into()],
                    &["cc@example.com".into()],
                    &["bcc@example.com".into()],
                )
                .unwrap(),
                "Subject".into(),
                "Body".into(),
                None,
                attachments,
            )
            .unwrap()
    }

    #[test]
    fn draft_and_attachments_are_immutable_and_bcc_is_envelope_only() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("note.txt");
        fs::write(&source, b"attachment bytes").unwrap();
        let store = base(root.path());
        let stored = draft(&store, &[source]);
        let loaded = store.load_draft(&stored.draft_ref).unwrap();
        assert_eq!(loaded.attachments.len(), 1);
        let raw = store.render_message(&stored.draft_ref).unwrap();
        let raw = String::from_utf8(raw).unwrap();
        assert!(raw.contains("To: to@example.com"));
        assert!(raw.contains("Cc: cc@example.com"));
        assert!(!raw.contains("bcc@example.com"));
        assert!(raw.contains("YXR0YWNobWVudCBieXRlcw=="));
    }

    #[test]
    fn draft_or_attachment_drift_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("note.txt");
        fs::write(&source, b"one").unwrap();
        let store = base(root.path());
        let stored = draft(&store, &[source]);
        let staged = store
            .attachments_dir()
            .join(&stored.draft_ref)
            .join("attachment-00.bin");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&staged, fs::Permissions::from_mode(0o600)).unwrap();
        }
        fs::write(staged, b"two").unwrap();
        assert!(matches!(
            store.load_draft(&stored.draft_ref),
            Err(EmailStoreError::AttachmentDrift)
        ));
    }

    #[test]
    fn unsafe_attachment_shapes_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let store = base(root.path());
        let directory = root.path().join("dir");
        fs::create_dir(&directory).unwrap();
        assert!(store
            .create_draft(
                AccountId::parse("personal").unwrap(),
                EmailAddress::parse("me@example.com").unwrap(),
                RecipientSet::parse(&["to@example.com".into()], &[], &[]).unwrap(),
                "Subject".into(),
                "Body".into(),
                None,
                &[directory],
            )
            .is_err());
    }

    #[test]
    fn gateway_journal_is_hash_chained_locked_and_metadata_only() {
        let root = tempfile::tempdir().unwrap();
        let store = GatewayStore::new(root.path().join("gateway"));
        let lock = store.lock().unwrap();
        assert!(matches!(store.lock(), Err(EmailStoreError::AlreadyExists)));
        let identity = GatewayMessageIdentity {
            account_id: "gateway".into(),
            folder: "INBOX".into(),
            uid_validity: 7,
            uid: 9,
        };
        store
            .append(GatewayEvent::Bootstrap {
                uid_validity: 7,
                high_water: 8,
            })
            .unwrap();
        let private = b"private-reply-body allowed@example.com";
        let digest = store.store_payload(&identity, private).unwrap();
        store
            .append(GatewayEvent::ResponseReady {
                identity: identity.clone(),
                draft_sha256: digest.clone(),
                deterministic_message_id: "<safe@example.com>".into(),
            })
            .unwrap();
        let journal = fs::read_to_string(store.journal_path()).unwrap();
        assert!(!journal.contains("private-reply-body"));
        assert!(!journal.contains("allowed@example.com"));
        assert_eq!(store.load_payload(&identity, &digest).unwrap(), private);
        drop(lock);
        assert!(store.lock().is_ok());

        let mut bytes = fs::read(store.journal_path()).unwrap();
        let index = bytes.iter().position(|byte| *byte == b'7').unwrap();
        bytes[index] = b'8';
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(store.journal_path(), fs::Permissions::from_mode(0o600)).unwrap();
        }
        fs::write(store.journal_path(), bytes).unwrap();
        assert!(store.load().is_err());
    }
}
