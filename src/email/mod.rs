pub mod gateway;
pub mod himalaya;
pub mod imap;
pub mod launchd;
pub mod mime;
pub mod secrets;
pub mod store;
pub mod types;

use crate::agent::registry::ToolExecutionResult;
use crate::config::EmailConfig;
use himalaya::{HimalayaAdapter, HimalayaError};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use store::{EmailStore, EmailStoreError, StoredDraft};
use types::{
    sha256_hex, AccountId, EmailAccountSummary, EmailAddress, EmailDeliveryReceipt,
    EmailDeliveryState, EmailDraftArgs, EmailEnvelopeSummary, EmailListArgs, EmailMessageView,
    EmailReadArgs, EmailReplyArgs, EmailSendArgs, ExternalCommunicationApproval,
    ExternalCommunicationPreview, FolderName, MessageId, RecipientSet, ThreadingFields,
    MAX_BODY_BYTES, MAX_LIST_RESULTS, MAX_MESSAGE_ID_BYTES, MAX_SUBJECT_BYTES,
};
use uuid::Uuid;

pub const EMAIL_TOOL_NAMES: [&str; 6] = [
    "email_accounts",
    "email_list",
    "email_read",
    "email_draft",
    "email_send",
    "email_reply",
];

#[derive(Debug, Clone, thiserror::Error)]
pub enum EmailError {
    #[error("email tools are disabled or unavailable")]
    Disabled,
    #[error("email request is invalid")]
    InvalidRequest,
    #[error("email account is not configured")]
    UnknownAccount,
    #[error("email account sender does not match configured identity")]
    SenderMismatch,
    #[error("email provider result is invalid")]
    InvalidProviderResult,
    #[error("email draft operation failed")]
    Store,
    #[error("email operation failed")]
    Adapter,
    #[error("exact external-communication approval is required")]
    ApprovalRequired,
    #[error("external-communication approval is invalid, expired, stale, or already used")]
    InvalidApproval,
}

impl From<EmailStoreError> for EmailError {
    fn from(_: EmailStoreError) -> Self {
        Self::Store
    }
}

impl From<HimalayaError> for EmailError {
    fn from(_: HimalayaError) -> Self {
        Self::Adapter
    }
}

#[derive(Clone)]
struct AccountRuntime {
    himalaya_account: String,
    from: EmailAddress,
}

impl std::fmt::Debug for AccountRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccountRuntime")
            .field("himalaya_account", &"[PRIVATE]")
            .field("from", &"[PRIVATE EMAIL ADDRESS]")
            .finish()
    }
}

#[derive(Clone)]
struct ApprovalEntry {
    approval: ExternalCommunicationApproval,
    draft_ref: String,
}

pub struct EmailManager {
    adapter: HimalayaAdapter,
    store: EmailStore,
    accounts: BTreeMap<String, AccountRuntime>,
    approvals: Mutex<HashMap<String, ApprovalEntry>>,
}

impl std::fmt::Debug for EmailManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EmailManager")
            .field("adapter", &self.adapter)
            .field("store", &self.store)
            .field("account_count", &self.accounts.len())
            .field(
                "approval_count",
                &self.approvals.lock().map(|map| map.len()).unwrap_or(0),
            )
            .finish()
    }
}

impl EmailManager {
    pub fn from_config(
        config: &EmailConfig,
        store_root: PathBuf,
    ) -> Result<Option<Arc<Self>>, EmailError> {
        if !config.enabled {
            return Ok(None);
        }
        let executable = config.executable.clone().ok_or(EmailError::Disabled)?;
        let adapter = HimalayaAdapter::new(
            executable,
            config.config_path.clone(),
            config.timeout_secs,
            config.max_output_bytes,
        )
        .map_err(|_| EmailError::Disabled)?;
        let mut accounts = BTreeMap::new();
        for (id, account) in &config.accounts {
            AccountId::parse(id.clone()).map_err(|_| EmailError::InvalidRequest)?;
            let from =
                EmailAddress::parse(&account.from).map_err(|_| EmailError::InvalidRequest)?;
            accounts.insert(
                id.clone(),
                AccountRuntime {
                    himalaya_account: account.himalaya_account.clone(),
                    from,
                },
            );
        }
        if accounts.is_empty() {
            return Err(EmailError::Disabled);
        }
        Ok(Some(Arc::new(Self {
            adapter,
            store: EmailStore::new(store_root),
            accounts,
            approvals: Mutex::new(HashMap::new()),
        })))
    }

    pub async fn accounts(&self) -> Result<Vec<EmailAccountSummary>, EmailError> {
        let value = self.adapter.accounts().await?;
        let rows = value
            .get("accounts")
            .and_then(Value::as_array)
            .ok_or(EmailError::InvalidProviderResult)?;
        if rows.len() > 64 {
            return Err(EmailError::InvalidProviderResult);
        }
        let mut reverse = BTreeMap::new();
        for (id, account) in &self.accounts {
            reverse.insert(account.himalaya_account.as_str(), id.as_str());
        }
        let mut output = Vec::new();
        for row in rows {
            let upstream = bounded_string(row.get("name"), 64)?;
            let Some(account_id) = reverse.get(upstream.as_str()) else {
                continue;
            };
            let backends = row
                .get("backends")
                .and_then(Value::as_array)
                .ok_or(EmailError::InvalidProviderResult)?
                .iter()
                .map(|value| bounded_string(Some(value), 32))
                .collect::<Result<Vec<_>, _>>()?;
            output.push(EmailAccountSummary {
                account_id: (*account_id).into(),
                default: row.get("default").and_then(Value::as_bool).unwrap_or(false),
                backends,
            });
        }
        output.sort_by(|left, right| left.account_id.cmp(&right.account_id));
        Ok(output)
    }

    pub async fn list(
        &self,
        args: &EmailListArgs,
    ) -> Result<Vec<EmailEnvelopeSummary>, EmailError> {
        let account_id =
            AccountId::parse(args.account_id.clone()).map_err(|_| EmailError::InvalidRequest)?;
        let folder =
            FolderName::parse(args.folder.clone()).map_err(|_| EmailError::InvalidRequest)?;
        if args.page == 0 || args.page_size == 0 || args.page_size > MAX_LIST_RESULTS {
            return Err(EmailError::InvalidRequest);
        }
        let account = self.account(&account_id)?;
        let value = self
            .adapter
            .list(
                &account.himalaya_account,
                folder.as_str(),
                args.page,
                args.page_size,
            )
            .await?;
        let rows = value
            .get("envelopes")
            .and_then(Value::as_array)
            .ok_or(EmailError::InvalidProviderResult)?;
        if rows.len() > args.page_size as usize {
            return Err(EmailError::InvalidProviderResult);
        }
        rows.iter()
            .map(|row| {
                let message_id = bounded_string(row.get("id"), MAX_MESSAGE_ID_BYTES)?;
                MessageId::parse(message_id.clone())
                    .map_err(|_| EmailError::InvalidProviderResult)?;
                let subject = optional_bounded_string(row.get("subject"), MAX_SUBJECT_BYTES)?;
                let from = parse_address_values(row.get("from"))?;
                let to = parse_address_values(row.get("to"))?;
                let size = row.get("size").and_then(Value::as_u64).unwrap_or(0);
                if size > 100 * 1024 * 1024 {
                    return Err(EmailError::InvalidProviderResult);
                }
                Ok(EmailEnvelopeSummary {
                    account_id: account_id.as_str().into(),
                    folder: folder.as_str().into(),
                    message_id,
                    subject,
                    from,
                    to,
                    date: row
                        .get("date")
                        .and_then(Value::as_str)
                        .map(|value| value.chars().take(64).collect()),
                    size,
                    has_attachment: row.get("has-attachment").and_then(Value::as_bool),
                })
            })
            .collect()
    }

    pub async fn read(&self, args: &EmailReadArgs) -> Result<EmailMessageView, EmailError> {
        let account_id =
            AccountId::parse(args.account_id.clone()).map_err(|_| EmailError::InvalidRequest)?;
        let folder =
            FolderName::parse(args.folder.clone()).map_err(|_| EmailError::InvalidRequest)?;
        let message_id =
            MessageId::parse(args.message_id.clone()).map_err(|_| EmailError::InvalidRequest)?;
        let account = self.account(&account_id)?;
        let value = self
            .adapter
            .read(
                &account.himalaya_account,
                folder.as_str(),
                message_id.as_str(),
            )
            .await?;
        parse_message_view(&value, &account_id, &folder, &message_id)
    }

    pub async fn draft(&self, args: EmailDraftArgs) -> Result<StoredDraft, EmailError> {
        let account_id =
            AccountId::parse(args.account_id).map_err(|_| EmailError::InvalidRequest)?;
        let from = EmailAddress::parse(args.from).map_err(|_| EmailError::InvalidRequest)?;
        let account = self.account(&account_id)?;
        if !from.as_str().eq_ignore_ascii_case(account.from.as_str()) {
            return Err(EmailError::SenderMismatch);
        }
        let recipients = RecipientSet::parse(&args.to, &args.cc, &args.bcc)
            .map_err(|_| EmailError::InvalidRequest)?;
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            store.create_draft(
                account_id,
                from,
                recipients,
                args.subject,
                args.body,
                None,
                &args.attachments,
            )
        })
        .await
        .map_err(|_| EmailError::Store)?
        .map_err(EmailError::from)
    }

    pub async fn prepare_reply(&self, args: EmailReplyArgs) -> Result<StoredDraft, EmailError> {
        let view = self
            .read(&EmailReadArgs {
                account_id: args.account_id.clone(),
                folder: args.folder.clone(),
                message_id: args.message_id.clone(),
            })
            .await?;
        let account_id =
            AccountId::parse(args.account_id).map_err(|_| EmailError::InvalidRequest)?;
        let from = EmailAddress::parse(args.from).map_err(|_| EmailError::InvalidRequest)?;
        let account = self.account(&account_id)?;
        if !from.as_str().eq_ignore_ascii_case(account.from.as_str()) {
            return Err(EmailError::SenderMismatch);
        }
        let to = if args.to.is_empty() {
            view.from.clone()
        } else {
            args.to
        };
        let recipients = RecipientSet::parse(&to, &args.cc, &args.bcc)
            .map_err(|_| EmailError::InvalidRequest)?;
        let source_id = view
            .rfc_message_id
            .as_deref()
            .ok_or(EmailError::InvalidProviderResult)?;
        let source_id = MessageId::parse(normalize_rfc_message_id(source_id))
            .map_err(|_| EmailError::InvalidProviderResult)?;
        let mut references = view
            .references
            .iter()
            .map(|value| MessageId::parse(normalize_rfc_message_id(value)))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| EmailError::InvalidProviderResult)?;
        if references.len() >= 20 {
            references.drain(..references.len() - 19);
        }
        if references.last() != Some(&source_id) {
            references.push(source_id.clone());
        }
        let subject = if view.subject.to_ascii_lowercase().starts_with("re:") {
            view.subject
        } else {
            format!("Re: {}", view.subject)
        };
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            store.create_draft(
                account_id,
                from,
                recipients,
                subject,
                args.body,
                Some(ThreadingFields {
                    in_reply_to: source_id,
                    references,
                }),
                &args.attachments,
            )
        })
        .await
        .map_err(|_| EmailError::Store)?
        .map_err(EmailError::from)
    }

    pub async fn preview(
        &self,
        draft_ref: &str,
    ) -> Result<ExternalCommunicationPreview, EmailError> {
        let store = self.store.clone();
        let draft_ref = draft_ref.to_string();
        tokio::task::spawn_blocking(move || store.preview(&draft_ref))
            .await
            .map_err(|_| EmailError::Store)?
            .map_err(EmailError::from)
    }

    pub async fn authorize_once(
        &self,
        draft_ref: &str,
        tool_call_id: &str,
        session_id: &str,
        ttl: std::time::Duration,
    ) -> Result<String, EmailError> {
        if tool_call_id.is_empty()
            || tool_call_id.len() > 256
            || session_id.is_empty()
            || session_id.len() > 256
            || ttl.is_zero()
            || ttl > std::time::Duration::from_secs(300)
        {
            return Err(EmailError::InvalidApproval);
        }
        let store = self.store.clone();
        let draft_ref_owned = draft_ref.to_string();
        let draft = tokio::task::spawn_blocking(move || store.load_draft(&draft_ref_owned))
            .await
            .map_err(|_| EmailError::Store)?
            .map_err(EmailError::from)?;
        let expires_at_unix_ms = chrono::Utc::now().timestamp_millis()
            + i64::try_from(ttl.as_millis()).map_err(|_| EmailError::InvalidApproval)?;
        let approval = ExternalCommunicationApproval {
            account_id: draft.account_id.clone(),
            recipients: draft.recipients.clone(),
            draft_digest: draft.digest(),
            attachment_digests: draft
                .attachments
                .iter()
                .map(|attachment| attachment.sha256.clone())
                .collect(),
            tool_call_id: tool_call_id.into(),
            session_id: session_id.into(),
            expires_at_unix_ms,
        };
        let nonce = format!("approval_{}", Uuid::new_v4().simple());
        self.approvals
            .lock()
            .map_err(|_| EmailError::InvalidApproval)?
            .insert(
                nonce.clone(),
                ApprovalEntry {
                    approval,
                    draft_ref: draft_ref.into(),
                },
            );
        Ok(nonce)
    }

    pub async fn send_approved(
        &self,
        args: &EmailSendArgs,
        nonce: &str,
        tool_call_id: &str,
        session_id: &str,
    ) -> Result<EmailDeliveryReceipt, EmailError> {
        self.adapter.ensure_compatible().await?;
        let entry = self
            .approvals
            .lock()
            .map_err(|_| EmailError::InvalidApproval)?
            .remove(nonce)
            .ok_or(EmailError::InvalidApproval)?;
        if entry.draft_ref != args.draft_ref
            || entry.approval.tool_call_id != tool_call_id
            || entry.approval.session_id != session_id
            || chrono::Utc::now().timestamp_millis() > entry.approval.expires_at_unix_ms
        {
            return Err(EmailError::InvalidApproval);
        }
        let store = self.store.clone();
        let draft_ref = args.draft_ref.clone();
        let draft = tokio::task::spawn_blocking(move || store.load_draft(&draft_ref))
            .await
            .map_err(|_| EmailError::Store)?
            .map_err(EmailError::from)?;
        if draft.digest() != entry.approval.draft_digest
            || draft.recipients != entry.approval.recipients
            || draft
                .attachments
                .iter()
                .map(|attachment| attachment.sha256.as_str())
                .ne(entry.approval.attachment_digests.iter().map(String::as_str))
        {
            return Err(EmailError::InvalidApproval);
        }
        let account = self.account(&draft.account_id)?.clone();
        let store = self.store.clone();
        let render_ref = args.draft_ref.clone();
        let message = tokio::task::spawn_blocking(move || store.render_message(&render_ref))
            .await
            .map_err(|_| EmailError::Store)?
            .map_err(EmailError::from)?;
        let store = self.store.clone();
        let draft_digest = draft.digest();
        let account_id = draft.account_id.as_str().to_string();
        let recipient_count = draft.recipients.count();
        tokio::task::spawn_blocking({
            let draft_digest = draft_digest.clone();
            let account_id = account_id.clone();
            move || {
                store.record_delivery(
                    &draft_digest,
                    EmailDeliveryState::NotStarted,
                    &account_id,
                    recipient_count,
                    "send_started",
                )
            }
        })
        .await
        .map_err(|_| EmailError::Store)?
        .map_err(EmailError::from)?;
        let state = match self
            .adapter
            .send(
                &account.himalaya_account,
                &draft.from,
                &draft.recipients,
                message,
            )
            .await
        {
            Ok(()) => EmailDeliveryState::Delivered,
            Err(_) => EmailDeliveryState::DeliveryUnknown,
        };
        let store = self.store.clone();
        let phase = match state {
            EmailDeliveryState::Delivered => "delivered",
            EmailDeliveryState::DeliveryUnknown => "delivery_unknown",
            EmailDeliveryState::NotStarted => unreachable!(),
        };
        tokio::task::spawn_blocking(move || {
            store.record_delivery(&draft_digest, state, &account_id, recipient_count, phase)
        })
        .await
        .map_err(|_| EmailError::Store)?
        .map_err(EmailError::from)
    }

    pub async fn execute_read_or_draft(
        &self,
        name: &str,
        arguments: &Value,
    ) -> ToolExecutionResult {
        let result = match name {
            "email_accounts" => self.accounts().await.and_then(|value| {
                serde_json::to_string(&value).map_err(|_| EmailError::InvalidProviderResult)
            }),
            "email_list" => match parse_args::<EmailListArgs>(arguments) {
                Ok(args) => self.list(&args).await,
                Err(error) => Err(error),
            }
            .and_then(|value| {
                serde_json::to_string(&value).map_err(|_| EmailError::InvalidProviderResult)
            }),
            "email_read" => match parse_args::<EmailReadArgs>(arguments) {
                Ok(args) => self.read(&args).await,
                Err(error) => Err(error),
            }
            .and_then(|value| {
                serde_json::to_string(&value).map_err(|_| EmailError::InvalidProviderResult)
            }),
            "email_draft" => match parse_args::<EmailDraftArgs>(arguments) {
                Ok(args) => self.draft(args).await,
                Err(error) => Err(error),
            }
            .and_then(|draft| {
                serde_json::to_string(&json!({
                    "draft_ref": draft.draft_ref,
                    "draft_sha256": draft.draft_sha256,
                    "draft_bytes": draft.draft_bytes,
                }))
                .map_err(|_| EmailError::InvalidProviderResult)
            }),
            _ => Err(EmailError::InvalidRequest),
        };
        match result {
            Ok(live) => ToolExecutionResult::ExternalPrivateSuccess {
                live,
                persistent: private_result_marker(name),
            },
            Err(error) => ToolExecutionResult::Failure(error.to_string()),
        }
    }

    fn account(&self, account_id: &AccountId) -> Result<&AccountRuntime, EmailError> {
        self.accounts
            .get(account_id.as_str())
            .ok_or(EmailError::UnknownAccount)
    }
}

fn parse_args<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, EmailError> {
    serde_json::from_value(value.clone()).map_err(|_| EmailError::InvalidRequest)
}

fn parse_message_view(
    value: &Value,
    account_id: &AccountId,
    folder: &FolderName,
    message_id: &MessageId,
) -> Result<EmailMessageView, EmailError> {
    let header_source = value
        .get("parts")
        .and_then(Value::as_array)
        .and_then(|parts| parts.first())
        .unwrap_or(value);
    let headers = collect_headers(header_source)?;
    let from = parse_header_addresses(headers.get("from"))?;
    let to = parse_header_addresses(headers.get("to"))?;
    let cc = parse_header_addresses(headers.get("cc"))?;
    let subject = headers
        .get("subject")
        .cloned()
        .or_else(|| {
            value
                .get("subject")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();
    if subject.len() > MAX_SUBJECT_BYTES {
        return Err(EmailError::InvalidProviderResult);
    }
    let body = value
        .get("body")
        .or_else(|| value.get("text_body"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| extract_first_text_body(value))
        .ok_or(EmailError::InvalidProviderResult)?;
    if body.len() > MAX_BODY_BYTES {
        return Err(EmailError::InvalidProviderResult);
    }
    let rfc_message_id = headers.get("message-id").cloned().or_else(|| {
        value
            .get("message_id")
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    if rfc_message_id
        .as_ref()
        .is_some_and(|id| id.len() > MAX_MESSAGE_ID_BYTES)
    {
        return Err(EmailError::InvalidProviderResult);
    }
    let references = headers
        .get("references")
        .map(|value| {
            value
                .split_ascii_whitespace()
                .take(20)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let attachment_count = value
        .get("attachments")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if attachment_count > types::MAX_ATTACHMENTS {
        return Err(EmailError::InvalidProviderResult);
    }
    Ok(EmailMessageView {
        account_id: account_id.as_str().into(),
        folder: folder.as_str().into(),
        message_id: message_id.as_str().into(),
        from,
        to,
        cc,
        subject,
        rfc_message_id,
        references,
        body,
        attachment_count,
    })
}

fn collect_headers(value: &Value) -> Result<BTreeMap<String, String>, EmailError> {
    let mut headers = BTreeMap::new();
    if let Some(values) = value.get("headers").and_then(Value::as_array) {
        if values.len() > 128 {
            return Err(EmailError::InvalidProviderResult);
        }
        for header in values {
            let name = header
                .get("name")
                .and_then(Value::as_str)
                .ok_or(EmailError::InvalidProviderResult)?
                .to_ascii_lowercase()
                .replace('_', "-");
            if !matches!(
                name.as_str(),
                "from" | "to" | "cc" | "subject" | "message-id" | "references"
            ) {
                continue;
            }
            let raw = header
                .get("value")
                .ok_or(EmailError::InvalidProviderResult)?;
            let value = decode_header_value(raw)?;
            if name.len() > 64 || value.len() > 16 * 1024 || value.contains('\0') {
                return Err(EmailError::InvalidProviderResult);
            }
            if headers.insert(name, value).is_some() {
                return Err(EmailError::InvalidProviderResult);
            }
        }
    }
    for name in ["from", "to", "cc", "subject", "message-id", "references"] {
        if let Some(value) = value.get(name).and_then(Value::as_str) {
            if value.len() > 16 * 1024 || value.contains('\0') {
                return Err(EmailError::InvalidProviderResult);
            }
            headers.entry(name.into()).or_insert_with(|| value.into());
        }
    }
    Ok(headers)
}

fn extract_first_text_body(value: &Value) -> Option<String> {
    if let (Some(indices), Some(parts)) = (
        value.get("text_body").and_then(Value::as_array),
        value.get("parts").and_then(Value::as_array),
    ) {
        for index in indices.iter().take(32).filter_map(Value::as_u64) {
            let index = usize::try_from(index).ok()?;
            let text = parts
                .get(index)
                .and_then(|part| part.get("body"))
                .and_then(|body| body.get("Text"))
                .and_then(Value::as_str);
            if let Some(text) = text {
                return Some(text.to_string());
            }
        }
    }
    value
        .get("text_bodies")
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(|value| {
            value
                .as_str()
                .or_else(|| value.get("contents").and_then(Value::as_str))
                .or_else(|| value.get("text").and_then(Value::as_str))
        })
        .map(str::to_string)
}

fn decode_header_value(value: &Value) -> Result<String, EmailError> {
    if let Some(value) = value.as_str() {
        return Ok(value.into());
    }
    let object = value.as_object().ok_or(EmailError::InvalidProviderResult)?;
    if let Some(value) = object.get("Text").and_then(Value::as_str) {
        return Ok(value.into());
    }
    if let Some(values) = object.get("TextList").and_then(Value::as_array) {
        if values.len() > 128 {
            return Err(EmailError::InvalidProviderResult);
        }
        return values
            .iter()
            .map(|value| value.as_str().ok_or(EmailError::InvalidProviderResult))
            .collect::<Result<Vec<_>, _>>()
            .map(|values| values.join(" "));
    }
    if let Some(address) = object.get("Address") {
        let mut output = Vec::new();
        let variants = address
            .as_object()
            .ok_or(EmailError::InvalidProviderResult)?;
        if let Some(list) = variants.get("List").and_then(Value::as_array) {
            collect_serialized_addresses(list, &mut output)?;
        } else if let Some(groups) = variants.get("Group").and_then(Value::as_array) {
            for group in groups {
                let list = group
                    .get("addresses")
                    .and_then(Value::as_array)
                    .ok_or(EmailError::InvalidProviderResult)?;
                collect_serialized_addresses(list, &mut output)?;
            }
        } else {
            return Err(EmailError::InvalidProviderResult);
        }
        return Ok(output.join(","));
    }
    Err(EmailError::InvalidProviderResult)
}

fn collect_serialized_addresses(
    values: &[Value],
    output: &mut Vec<String>,
) -> Result<(), EmailError> {
    if output.len().saturating_add(values.len()) > types::MAX_RECIPIENTS {
        return Err(EmailError::InvalidProviderResult);
    }
    for value in values {
        let address = value
            .get("address")
            .and_then(Value::as_str)
            .ok_or(EmailError::InvalidProviderResult)?;
        let address =
            EmailAddress::parse(address).map_err(|_| EmailError::InvalidProviderResult)?;
        output.push(address.as_str().into());
    }
    Ok(())
}

fn parse_header_addresses(value: Option<&String>) -> Result<Vec<String>, EmailError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value.split(',').map(str::trim).collect::<Vec<_>>();
    if values.len() > types::MAX_RECIPIENTS {
        return Err(EmailError::InvalidProviderResult);
    }
    values
        .into_iter()
        .filter(|value| !value.is_empty())
        .map(|value| {
            EmailAddress::parse(value)
                .map(|address| address.as_str().to_string())
                .map_err(|_| EmailError::InvalidProviderResult)
        })
        .collect()
}

fn parse_address_values(value: Option<&Value>) -> Result<Vec<String>, EmailError> {
    let Some(Value::Array(values)) = value else {
        return Ok(Vec::new());
    };
    if values.len() > types::MAX_RECIPIENTS {
        return Err(EmailError::InvalidProviderResult);
    }
    values
        .iter()
        .map(|value| {
            let raw = value
                .as_str()
                .or_else(|| value.get("address").and_then(Value::as_str))
                .or_else(|| value.get("email").and_then(Value::as_str))
                .ok_or(EmailError::InvalidProviderResult)?;
            EmailAddress::parse(raw)
                .map(|address| address.as_str().to_string())
                .map_err(|_| EmailError::InvalidProviderResult)
        })
        .collect()
}

fn bounded_string(value: Option<&Value>, max: usize) -> Result<String, EmailError> {
    let value = value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= max && !value.contains('\0'))
        .ok_or(EmailError::InvalidProviderResult)?;
    Ok(value.into())
}

fn optional_bounded_string(value: Option<&Value>, max: usize) -> Result<String, EmailError> {
    match value.and_then(Value::as_str) {
        Some(value) if value.len() <= max && !value.contains('\0') => Ok(value.into()),
        Some(_) => Err(EmailError::InvalidProviderResult),
        None => Ok(String::new()),
    }
}

fn normalize_rfc_message_id(value: &str) -> String {
    value
        .trim()
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .unwrap_or(value.trim())
        .to_string()
}

pub fn is_email_tool(name: &str) -> bool {
    EMAIL_TOOL_NAMES.contains(&name)
}

pub fn is_outbound_tool(name: &str) -> bool {
    matches!(name, "email_send" | "email_reply")
}

pub fn private_arguments_marker(name: &str) -> String {
    serde_json::to_string(&json!({
        "external_private": true,
        "tool": if is_email_tool(name) { name } else { "email" },
        "content": "unavailable",
    }))
    .expect("private marker serializes")
}

pub fn private_result_marker(name: &str) -> String {
    format!("[External private {name} result unavailable after active tool loop]")
}

pub fn scrub_messages_in_place(messages: &mut [crate::app::Message]) {
    for message in messages {
        if let Some(calls) = message.tool_calls.as_mut() {
            for call in calls {
                if is_email_tool(&call.name) {
                    call.arguments = private_arguments_marker(&call.name);
                    if call.output.is_some() {
                        call.output = Some(private_result_marker(&call.name));
                    }
                }
            }
        }
    }
}

pub fn metadata_only(name: &str, arguments: &Value, result: Option<&str>) -> Value {
    let operation = name.strip_prefix("email_").unwrap_or("email");
    let account_digest = arguments
        .get("account_id")
        .and_then(Value::as_str)
        .map(|value| sha256_hex(value.as_bytes()));
    let recipient_count = ["to", "cc", "bcc"]
        .iter()
        .filter_map(|field| arguments.get(field).and_then(Value::as_array))
        .map(Vec::len)
        .sum::<usize>();
    json!({
        "tool_name": name,
        "email": {
            "external_private": true,
            "operation": operation,
            "account_digest": account_digest,
            "recipient_count": recipient_count,
            "body_bytes": arguments.get("body").and_then(Value::as_str).map(str::len),
            "attachment_count": arguments.get("attachments").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
            "outcome": result.map(|value| if value.starts_with('[') { "private" } else { "available" }),
        }
    })
}

/// Returns the collision-resistant commitment a service client must echo and
/// the metadata-only preview safe to place on the service wire.
pub fn service_approval_metadata(
    preview: &ExternalCommunicationPreview,
) -> Result<(String, Value), EmailError> {
    let exact = serde_json::to_vec(preview).map_err(|_| EmailError::InvalidProviderResult)?;
    let approval_digest = sha256_hex(&exact);
    let recipients = RecipientSet::parse(&preview.to, &preview.cc, &preview.bcc)
        .map_err(|_| EmailError::InvalidProviderResult)?;
    let attachment_bytes = preview
        .attachments
        .iter()
        .try_fold(0_u64, |total, attachment| {
            total.checked_add(attachment.size)
        })
        .ok_or(EmailError::InvalidProviderResult)?;
    Ok((
        approval_digest,
        json!({
            "account_id": preview.account_id,
            "recipient_count": preview.to.len() + preview.cc.len() + preview.bcc.len(),
            "recipient_set_sha256": recipients.digest(),
            "subject_bytes": preview.subject.len(),
            "subject_sha256": sha256_hex(preview.subject.as_bytes()),
            "body_bytes": preview.body_bytes,
            "body_sha256": preview.body_sha256,
            "draft_sha256": preview.draft_sha256,
            "attachment_count": preview.attachments.len(),
            "attachment_bytes": attachment_bytes,
            "attachment_sha256": preview.attachments.iter().map(|item| item.sha256.clone()).collect::<Vec<_>>(),
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EmailAccountConfig, EmailConfig};
    use std::fs;

    fn manager(root: &std::path::Path) -> Arc<EmailManager> {
        let executable = root.join("fake-himalaya");
        let config_path = root.join("himalaya.toml");
        fs::write(&config_path, "").unwrap();
        fs::write(
            &executable,
            "#!/bin/sh\nif [ \"${1-}\" = '--version' ]; then echo 'himalaya 2.0.0'; exit 0; fi\ncase \" $* \" in\n*' message read '*) echo '{\"from\":\"sender@example.com\",\"to\":\"me@example.com\",\"subject\":\"Topic\",\"message-id\":\"<source@example.com>\",\"references\":\"<older@example.com>\",\"body\":\"Source\"}' ;;\n*' smtp send '*) /bin/cat >/dev/null; echo '\"Message successfully sent\"' ;;\n*) echo '{\"accounts\":[]}' ;;\nesac\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut accounts = BTreeMap::new();
        accounts.insert(
            "personal".into(),
            EmailAccountConfig {
                himalaya_account: "upstream".into(),
                from: "me@example.com".into(),
            },
        );
        EmailManager::from_config(
            &EmailConfig {
                enabled: true,
                executable: Some(executable),
                config_path: Some(config_path),
                version: "2.0".into(),
                timeout_secs: 2,
                max_output_bytes: 64 * 1024,
                accounts,
                gateway: Default::default(),
            },
            root.join("email"),
        )
        .unwrap()
        .unwrap()
    }

    #[tokio::test]
    async fn approval_is_exact_single_use_and_bound_to_call_and_session() {
        let root = tempfile::tempdir().unwrap();
        let manager = manager(root.path());
        let draft = manager
            .draft(EmailDraftArgs {
                account_id: "personal".into(),
                from: "me@example.com".into(),
                to: vec!["to@example.com".into()],
                cc: vec![],
                bcc: vec![],
                subject: "Subject".into(),
                body: "Body".into(),
                attachments: vec![],
            })
            .await
            .unwrap();
        let nonce = manager
            .authorize_once(
                &draft.draft_ref,
                "call-1",
                "session-1",
                std::time::Duration::from_secs(30),
            )
            .await
            .unwrap();
        assert!(manager
            .send_approved(
                &EmailSendArgs {
                    draft_ref: draft.draft_ref.clone(),
                },
                &nonce,
                "wrong-call",
                "session-1",
            )
            .await
            .is_err());
        assert!(
            manager
                .send_approved(
                    &EmailSendArgs {
                        draft_ref: draft.draft_ref,
                    },
                    &nonce,
                    "call-1",
                    "session-1",
                )
                .await
                .is_err(),
            "mismatched attempt consumes the nonce"
        );
    }

    #[tokio::test]
    async fn reply_freezes_threading_before_approval() {
        let root = tempfile::tempdir().unwrap();
        let manager = manager(root.path());
        let draft = manager
            .prepare_reply(EmailReplyArgs {
                account_id: "personal".into(),
                folder: "INBOX".into(),
                message_id: "42".into(),
                from: "me@example.com".into(),
                to: vec![],
                cc: vec![],
                bcc: vec![],
                body: "Reply".into(),
                attachments: vec![],
            })
            .await
            .unwrap();
        let loaded = manager.store.load_draft(&draft.draft_ref).unwrap();
        assert_eq!(loaded.subject, "Re: Topic");
        assert_eq!(loaded.recipients.to[0].as_str(), "sender@example.com");
        assert_eq!(
            loaded.threading.unwrap().in_reply_to.as_str(),
            "source@example.com"
        );
    }

    #[test]
    fn metadata_payload_recursively_omits_private_sentinels() {
        let arguments = json!({
            "account_id": "private-account",
            "to": ["private-to@example.com"],
            "body": "private-body",
            "attachments": ["/private/path"]
        });
        let payload = metadata_only("email_draft", &arguments, Some("private-result"));
        let rendered = payload.to_string();
        for sentinel in [
            "private-account",
            "private-to@example.com",
            "private-body",
            "/private/path",
            "private-result",
        ] {
            assert!(!rendered.contains(sentinel));
        }
        assert!(rendered.contains("recipient_count"));
    }

    #[test]
    fn service_approval_is_digest_bound_and_metadata_only() {
        let preview = ExternalCommunicationPreview {
            account_id: "work".into(),
            to: vec!["private-to@example.com".into()],
            cc: vec![],
            bcc: vec!["private-bcc@example.com".into()],
            subject: "private subject sentinel".into(),
            body_bytes: 23,
            body_sha256: sha256_hex(b"private body sentinel"),
            draft_sha256: "d".repeat(64),
            attachments: vec![types::AttachmentPreview {
                basename: "private-name.pdf".into(),
                size: 42,
                sha256: "a".repeat(64),
            }],
        };
        let (digest, metadata) = service_approval_metadata(&preview).unwrap();
        let encoded = metadata.to_string();
        for private in [
            "private-to@example.com",
            "private-bcc@example.com",
            "private subject sentinel",
            "private body sentinel",
            "private-name.pdf",
        ] {
            assert!(!encoded.contains(private));
        }
        let mut changed = preview.clone();
        changed.to = vec!["different@example.com".into()];
        assert_ne!(
            digest,
            service_approval_metadata(&changed).unwrap().0,
            "the echoed service commitment must bind the exact recipients"
        );
    }

    #[test]
    fn terminal_in_memory_scrub_replaces_email_arguments_and_results() {
        let sentinel = "terminal-private-email-sentinel";
        let mut messages = vec![crate::app::Message {
            role: crate::app::Role::Assistant,
            content: String::new(),
            tool_calls: Some(vec![crate::app::ToolCall {
                id: "email-call".into(),
                name: "email_read".into(),
                arguments: json!({"body":sentinel}).to_string(),
                status: crate::app::ToolStatus::Done,
                output: Some(sentinel.into()),
            }]),
            thinking: None,
            timestamp: chrono::Utc::now(),
            response_items: Vec::new(),
        }];
        scrub_messages_in_place(&mut messages);
        let encoded = serde_json::to_string(&messages).unwrap();
        assert!(!encoded.contains(sentinel));
        assert!(encoded.contains("external_private"));
    }

    #[test]
    fn current_himalaya_v2_message_json_shape_is_parsed() {
        let value = json!({
            "html_body": [],
            "text_body": [1],
            "attachments": [2],
            "parts": [
                {
                    "headers": [
                        {"name":"from","value":{"Address":{"List":[{"name":"Sender","address":"sender@example.com"}]}}},
                        {"name":"to","value":{"Address":{"List":[{"name":null,"address":"me@example.com"}]}}},
                        {"name":"subject","value":{"Text":"Topic"}},
                        {"name":"message_id","value":{"Text":"source@example.com"}},
                        {"name":"references","value":{"TextList":["older@example.com","source@example.com"]}},
                        {"name":"content_type","value":{"ContentType":{"c_type":"multipart"}}}
                    ],
                    "body":{"Multipart":[1,2]}
                },
                {"headers":[],"body":{"Text":"Source body"}},
                {"headers":[],"body":{"Binary":[1,2,3]}}
            ]
        });
        let parsed = parse_message_view(
            &value,
            &AccountId::parse("work").unwrap(),
            &FolderName::parse("INBOX").unwrap(),
            &MessageId::parse("42").unwrap(),
        )
        .unwrap();
        assert_eq!(parsed.from, ["sender@example.com"]);
        assert_eq!(parsed.to, ["me@example.com"]);
        assert_eq!(parsed.subject, "Topic");
        assert_eq!(parsed.body, "Source body");
        assert_eq!(parsed.attachment_count, 1);
        assert_eq!(
            parsed.references,
            ["older@example.com", "source@example.com"]
        );
    }
}
