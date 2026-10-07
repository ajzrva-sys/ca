use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

pub const MAX_ACCOUNT_BYTES: usize = 64;
pub const MAX_FOLDER_BYTES: usize = 512;
pub const MAX_MESSAGE_ID_BYTES: usize = 512;
pub const MAX_ADDRESS_BYTES: usize = 320;
pub const MAX_RECIPIENTS: usize = 100;
pub const MAX_SUBJECT_BYTES: usize = 998;
pub const MAX_BODY_BYTES: usize = 256 * 1024;
pub const MAX_ATTACHMENTS: usize = 16;
pub const MAX_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_TOTAL_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;
pub const MAX_LIST_RESULTS: u32 = 100;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum EmailTypeError {
    #[error("email account identifier is invalid")]
    InvalidAccount,
    #[error("email folder is invalid")]
    InvalidFolder,
    #[error("email message identifier is invalid")]
    InvalidMessageId,
    #[error("email address is invalid")]
    InvalidAddress,
    #[error("email recipient set is invalid")]
    InvalidRecipients,
    #[error("email subject is invalid")]
    InvalidSubject,
    #[error("email body is invalid")]
    InvalidBody,
    #[error("email attachment set is invalid")]
    InvalidAttachments,
}

fn safe_bounded(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && !value
            .chars()
            .any(|character| character.is_control() || matches!(character, '\r' | '\n' | '\0'))
}

macro_rules! bounded_string {
    ($name:ident, $max:expr, $error:ident) => {
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, EmailTypeError> {
                let value = value.into();
                if !safe_bounded(&value, $max) {
                    return Err(EmailTypeError::$error);
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("[PRIVATE]")
            }
        }
    };
}

bounded_string!(AccountId, MAX_ACCOUNT_BYTES, InvalidAccount);
bounded_string!(FolderName, MAX_FOLDER_BYTES, InvalidFolder);
bounded_string!(MessageId, MAX_MESSAGE_ID_BYTES, InvalidMessageId);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EmailAddress(String);

impl EmailAddress {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, EmailTypeError> {
        let value = value.as_ref().trim();
        if !safe_bounded(value, MAX_ADDRESS_BYTES)
            || value.contains(['<', '>', ',', ';', ' '])
            || value.matches('@').count() != 1
        {
            return Err(EmailTypeError::InvalidAddress);
        }
        let (local, domain) = value
            .rsplit_once('@')
            .ok_or(EmailTypeError::InvalidAddress)?;
        if local.is_empty()
            || domain.is_empty()
            || local.starts_with('.')
            || local.ends_with('.')
            || local.contains("..")
            || domain.starts_with('.')
            || domain.ends_with('.')
            || domain.contains("..")
            || !domain.contains('.')
            || !local.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(
                        character,
                        '!' | '#'
                            | '$'
                            | '%'
                            | '&'
                            | '\''
                            | '*'
                            | '+'
                            | '-'
                            | '/'
                            | '='
                            | '?'
                            | '^'
                            | '_'
                            | '`'
                            | '{'
                            | '|'
                            | '}'
                            | '~'
                            | '.'
                    )
            })
            || !domain.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '.')
            })
        {
            return Err(EmailTypeError::InvalidAddress);
        }
        Ok(Self(format!("{}@{}", local, domain.to_ascii_lowercase())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for EmailAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[PRIVATE EMAIL ADDRESS]")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipientSet {
    pub to: Vec<EmailAddress>,
    pub cc: Vec<EmailAddress>,
    pub bcc: Vec<EmailAddress>,
}

impl fmt::Debug for RecipientSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RecipientSet")
            .field("to_count", &self.to.len())
            .field("cc_count", &self.cc.len())
            .field("bcc_count", &self.bcc.len())
            .finish()
    }
}

impl RecipientSet {
    pub fn parse(to: &[String], cc: &[String], bcc: &[String]) -> Result<Self, EmailTypeError> {
        let count = to.len().saturating_add(cc.len()).saturating_add(bcc.len());
        if count == 0 || count > MAX_RECIPIENTS {
            return Err(EmailTypeError::InvalidRecipients);
        }
        let mut seen = BTreeSet::new();
        let parse_group = |values: &[String], seen: &mut BTreeSet<String>| {
            let mut parsed = Vec::with_capacity(values.len());
            for value in values {
                let address = EmailAddress::parse(value)?;
                let folded = address.as_str().to_ascii_lowercase();
                if seen.insert(folded) {
                    parsed.push(address);
                }
            }
            parsed.sort();
            Ok::<_, EmailTypeError>(parsed)
        };
        let to = parse_group(to, &mut seen)?;
        let cc = parse_group(cc, &mut seen)?;
        let bcc = parse_group(bcc, &mut seen)?;
        if to.is_empty() && cc.is_empty() && bcc.is_empty() {
            return Err(EmailTypeError::InvalidRecipients);
        }
        Ok(Self { to, cc, bcc })
    }

    pub fn count(&self) -> usize {
        self.to.len() + self.cc.len() + self.bcc.len()
    }

    pub fn all(&self) -> impl Iterator<Item = &EmailAddress> {
        self.to.iter().chain(&self.cc).chain(&self.bcc)
    }

    pub fn digest(&self) -> String {
        sha256_hex(&serde_json::to_vec(self).expect("recipient set serializes"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadingFields {
    pub in_reply_to: MessageId,
    pub references: Vec<MessageId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StagedAttachment {
    pub basename: String,
    pub size: u64,
    pub sha256: String,
    pub stored_name: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmailDraftV1 {
    pub schema: String,
    pub account_id: AccountId,
    pub from: EmailAddress,
    pub recipients: RecipientSet,
    pub subject: String,
    pub headers: BTreeMap<String, String>,
    pub body: String,
    pub threading: Option<ThreadingFields>,
    pub attachments: Vec<StagedAttachment>,
}

impl fmt::Debug for EmailDraftV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EmailDraftV1")
            .field("schema", &self.schema)
            .field("account_id", &"[PRIVATE]")
            .field("recipients", &self.recipients)
            .field("subject_bytes", &self.subject.len())
            .field("body_bytes", &self.body.len())
            .field("attachment_count", &self.attachments.len())
            .finish()
    }
}

impl EmailDraftV1 {
    pub fn new(
        account_id: AccountId,
        from: EmailAddress,
        recipients: RecipientSet,
        subject: String,
        body: String,
        threading: Option<ThreadingFields>,
        attachments: Vec<StagedAttachment>,
    ) -> Result<Self, EmailTypeError> {
        if subject.len() > MAX_SUBJECT_BYTES
            || subject.chars().any(|character| character.is_control())
        {
            return Err(EmailTypeError::InvalidSubject);
        }
        if body.len() > MAX_BODY_BYTES || body.contains('\0') {
            return Err(EmailTypeError::InvalidBody);
        }
        if attachments.len() > MAX_ATTACHMENTS
            || attachments.iter().any(|attachment| {
                attachment.size > MAX_ATTACHMENT_BYTES
                    || attachment.basename.is_empty()
                    || attachment.basename.len() > 255
            })
            || attachments
                .iter()
                .map(|attachment| attachment.size)
                .try_fold(0_u64, u64::checked_add)
                .is_none_or(|total| total > MAX_TOTAL_ATTACHMENT_BYTES)
        {
            return Err(EmailTypeError::InvalidAttachments);
        }
        Ok(Self {
            schema: "ca.email-draft.v1".into(),
            account_id,
            from,
            recipients,
            subject,
            headers: BTreeMap::new(),
            body,
            threading,
            attachments,
        })
    }

    pub fn digest(&self) -> String {
        sha256_hex(&serde_json::to_vec(self).expect("email draft serializes"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmailAccountsArgs {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmailListArgs {
    pub account_id: String,
    pub folder: String,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
}

fn default_page() -> u32 {
    1
}

fn default_page_size() -> u32 {
    25
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmailReadArgs {
    pub account_id: String,
    pub folder: String,
    pub message_id: String,
}

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmailDraftArgs {
    pub account_id: String,
    pub from: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    #[serde(default)]
    pub attachments: Vec<PathBuf>,
}

impl fmt::Debug for EmailDraftArgs {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EmailDraftArgs")
            .field("account_id", &"[PRIVATE]")
            .field("to_count", &self.to.len())
            .field("cc_count", &self.cc.len())
            .field("bcc_count", &self.bcc.len())
            .field("subject_bytes", &self.subject.len())
            .field("body_bytes", &self.body.len())
            .field("attachment_count", &self.attachments.len())
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmailSendArgs {
    pub draft_ref: String,
}

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmailReplyArgs {
    pub account_id: String,
    pub folder: String,
    pub message_id: String,
    pub from: String,
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub body: String,
    #[serde(default)]
    pub attachments: Vec<PathBuf>,
}

impl fmt::Debug for EmailReplyArgs {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EmailReplyArgs")
            .field("account_id", &"[PRIVATE]")
            .field("folder", &"[PRIVATE]")
            .field("message_id", &"[PRIVATE]")
            .field(
                "recipient_count",
                &(self.to.len() + self.cc.len() + self.bcc.len()),
            )
            .field("body_bytes", &self.body.len())
            .field("attachment_count", &self.attachments.len())
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmailAccountSummary {
    pub account_id: String,
    pub default: bool,
    pub backends: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmailEnvelopeSummary {
    pub account_id: String,
    pub folder: String,
    pub message_id: String,
    pub subject: String,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub date: Option<String>,
    pub size: u64,
    pub has_attachment: Option<bool>,
}

impl fmt::Debug for EmailEnvelopeSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EmailEnvelopeSummary")
            .field("account_id", &"[PRIVATE]")
            .field("folder", &"[PRIVATE]")
            .field("message_id", &"[PRIVATE]")
            .field("subject_bytes", &self.subject.len())
            .field("from_count", &self.from.len())
            .field("to_count", &self.to.len())
            .field("size", &self.size)
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmailMessageView {
    pub account_id: String,
    pub folder: String,
    pub message_id: String,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub rfc_message_id: Option<String>,
    pub references: Vec<String>,
    pub body: String,
    pub attachment_count: usize,
}

impl fmt::Debug for EmailMessageView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EmailMessageView")
            .field("account_id", &"[PRIVATE]")
            .field("folder", &"[PRIVATE]")
            .field("message_id", &"[PRIVATE]")
            .field(
                "address_count",
                &(self.from.len() + self.to.len() + self.cc.len()),
            )
            .field("subject_bytes", &self.subject.len())
            .field("body_bytes", &self.body.len())
            .field("attachment_count", &self.attachment_count)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmailDeliveryState {
    NotStarted,
    Delivered,
    DeliveryUnknown,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternalCommunicationApproval {
    pub account_id: AccountId,
    pub recipients: RecipientSet,
    pub draft_digest: String,
    pub attachment_digests: Vec<String>,
    pub tool_call_id: String,
    pub session_id: String,
    pub expires_at_unix_ms: i64,
}

impl fmt::Debug for ExternalCommunicationApproval {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalCommunicationApproval")
            .field("account_id", &"[PRIVATE]")
            .field("recipient_count", &self.recipients.count())
            .field("draft_digest", &self.draft_digest)
            .field("attachment_count", &self.attachment_digests.len())
            .field("tool_call_id", &self.tool_call_id)
            .field("session_id", &"[PRIVATE]")
            .field("expires_at_unix_ms", &self.expires_at_unix_ms)
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternalCommunicationPreview {
    pub account_id: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_bytes: usize,
    pub body_sha256: String,
    pub draft_sha256: String,
    pub attachments: Vec<AttachmentPreview>,
}

impl fmt::Debug for ExternalCommunicationPreview {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalCommunicationPreview")
            .field("account_id", &"[PRIVATE]")
            .field("to_count", &self.to.len())
            .field("cc_count", &self.cc.len())
            .field("bcc_count", &self.bcc.len())
            .field("subject_bytes", &self.subject.len())
            .field("body_bytes", &self.body_bytes)
            .field("body_sha256", &self.body_sha256)
            .field("draft_sha256", &self.draft_sha256)
            .field("attachment_count", &self.attachments.len())
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttachmentPreview {
    pub basename: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmailDeliveryReceipt {
    pub receipt_id: String,
    pub state: EmailDeliveryState,
    pub account_digest: String,
    pub recipient_count: usize,
    pub timestamp: String,
    pub message_id_digest: Option<String>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_canonicalization_and_recipient_dedup_are_closed() {
        assert_eq!(
            EmailAddress::parse("User@EXAMPLE.com").unwrap().as_str(),
            "User@example.com"
        );
        for invalid in [
            "Name <user@example.com>",
            "a@example.com\r\nBcc:x@example.com",
            "a@@example.com",
            "a@localhost",
        ] {
            assert!(EmailAddress::parse(invalid).is_err(), "{invalid}");
        }
        let recipients = RecipientSet::parse(
            &["a@example.com".into(), "a@EXAMPLE.com".into()],
            &["b@example.com".into()],
            &[],
        )
        .unwrap();
        assert_eq!(recipients.count(), 2);
    }

    #[test]
    fn private_debug_omits_addresses_bodies_and_subjects() {
        let args = EmailDraftArgs {
            account_id: "private-account".into(),
            from: "from@example.com".into(),
            to: vec!["to@example.com".into()],
            cc: vec![],
            bcc: vec![],
            subject: "private-subject".into(),
            body: "private-body".into(),
            attachments: vec![],
        };
        let rendered = format!("{args:?}");
        for sentinel in [
            "private-account",
            "from@example.com",
            "to@example.com",
            "private-subject",
            "private-body",
        ] {
            assert!(!rendered.contains(sentinel));
        }
    }

    #[test]
    fn draft_rejects_header_injection_and_bounds() {
        let recipients = RecipientSet::parse(&["to@example.com".into()], &[], &[]).unwrap();
        let account = AccountId::parse("personal").unwrap();
        let from = EmailAddress::parse("from@example.com").unwrap();
        assert!(EmailDraftV1::new(
            account.clone(),
            from.clone(),
            recipients.clone(),
            "ok\r\nBcc: bad@example.com".into(),
            "body".into(),
            None,
            vec![],
        )
        .is_err());
        assert!(EmailDraftV1::new(
            account,
            from,
            recipients,
            "subject".into(),
            "x".repeat(MAX_BODY_BYTES + 1),
            None,
            vec![],
        )
        .is_err());
    }
}
