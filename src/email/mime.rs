use crate::email::types::{EmailAddress, MAX_SUBJECT_BYTES};
use mail_parser::{HeaderValue, MessageParser, MimeHeaders};
use serde::{Deserialize, Serialize};

const MAX_HEADERS: usize = 256;
const MAX_PARTS: usize = 128;
const MAX_ATTACHMENTS: usize = 32;
const MAX_MESSAGE_ID: usize = 512;
const MAX_REFERENCES: usize = 20;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum MimeError {
    #[error("email MIME input exceeds a safety bound")]
    Bound,
    #[error("email MIME input is malformed")]
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AttachmentMetadata {
    pub media_type: String,
    pub decoded_bytes: usize,
    pub named: bool,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ParsedInbound {
    pub sender: EmailAddress,
    pub subject: String,
    pub body: String,
    pub message_id: Option<String>,
    pub references: Vec<String>,
    pub attachments: Vec<AttachmentMetadata>,
    pub auto_submitted: bool,
    pub list_or_bulk: bool,
}

impl std::fmt::Debug for ParsedInbound {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ParsedInbound")
            .field("sender", &"[PRIVATE EMAIL ADDRESS]")
            .field("subject", &"[PRIVATE SUBJECT]")
            .field("body", &"[PRIVATE BODY]")
            .field("has_message_id", &self.message_id.is_some())
            .field("reference_count", &self.references.len())
            .field("attachments", &self.attachments)
            .field("auto_submitted", &self.auto_submitted)
            .field("list_or_bulk", &self.list_or_bulk)
            .finish()
    }
}

pub fn parse_inbound(
    raw: &[u8],
    max_raw_bytes: usize,
    max_body_bytes: usize,
) -> Result<ParsedInbound, MimeError> {
    if raw.is_empty() || raw.len() > max_raw_bytes {
        return Err(MimeError::Bound);
    }
    let message = MessageParser::default()
        .parse(raw)
        .ok_or(MimeError::Invalid)?;
    if message.parts.len() > MAX_PARTS
        || message
            .parts
            .iter()
            .map(|part| part.headers.len())
            .sum::<usize>()
            > MAX_HEADERS
        || message.attachment_count() > MAX_ATTACHMENTS
    {
        return Err(MimeError::Bound);
    }
    let sender = message
        .from()
        .and_then(|addresses| addresses.first())
        .and_then(|address| address.address.as_deref())
        .ok_or(MimeError::Invalid)
        .and_then(|value| EmailAddress::parse(value).map_err(|_| MimeError::Invalid))?;
    let subject = bounded_text(message.subject().unwrap_or(""), MAX_SUBJECT_BYTES)?;
    // mail-parser converts an HTML-only message to local plain text. It does
    // not fetch remote resources; attachment bodies are never copied here.
    let body = bounded_text(
        message.body_text(0).as_deref().unwrap_or(""),
        max_body_bytes,
    )?;
    let message_id = message.message_id().map(bounded_message_id).transpose()?;
    let references = header_values(message.references())
        .into_iter()
        .take(MAX_REFERENCES + 1)
        .map(bounded_message_id)
        .collect::<Result<Vec<_>, _>>()?;
    if references.len() > MAX_REFERENCES {
        return Err(MimeError::Bound);
    }
    let attachments = message
        .attachments()
        .map(|part| AttachmentMetadata {
            media_type: part
                .content_type()
                .map(|content_type| {
                    format!(
                        "{}/{}",
                        content_type.c_type,
                        content_type.c_subtype.as_deref().unwrap_or("octet-stream")
                    )
                })
                .filter(|value| value.len() <= 128)
                .unwrap_or_else(|| "application/octet-stream".into()),
            decoded_bytes: part.len(),
            named: part.attachment_name().is_some(),
        })
        .collect();
    let auto_submitted = header_is_automated(message.header_raw("Auto-Submitted"));
    let precedence = message
        .header_raw("Precedence")
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let list_or_bulk = !matches!(message.list_id(), HeaderValue::Empty)
        || matches!(precedence.as_str(), "bulk" | "list" | "junk");
    Ok(ParsedInbound {
        sender,
        subject,
        body,
        message_id,
        references,
        attachments,
        auto_submitted,
        list_or_bulk,
    })
}

fn bounded_text(value: &str, limit: usize) -> Result<String, MimeError> {
    if value.len() > limit || value.chars().any(|character| character == '\0') {
        return Err(MimeError::Bound);
    }
    Ok(value.replace("\r\n", "\n").replace('\r', "\n"))
}

fn bounded_message_id(value: &str) -> Result<String, MimeError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > MAX_MESSAGE_ID
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(MimeError::Invalid);
    }
    Ok(value.to_owned())
}

fn header_values<'a>(value: &'a HeaderValue<'a>) -> Vec<&'a str> {
    value
        .as_text_list()
        .map(|values| values.iter().map(AsRef::as_ref).collect())
        .or_else(|| value.as_text().map(|value| vec![value]))
        .unwrap_or_default()
}

fn header_is_automated(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        let value = value.trim();
        !value.eq_ignore_ascii_case("no") && !value.is_empty()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_mail_and_omits_attachment_bytes() {
        let raw = b"From: Allowed <allowed@example.com>\r\nSubject: Hello\r\nMessage-ID: <one@example.com>\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nreply text\r\n--x\r\nContent-Type: application/octet-stream; name=secret.bin\r\nContent-Disposition: attachment; filename=secret.bin\r\nContent-Transfer-Encoding: base64\r\n\r\nc2VudGluZWwtYXR0YWNobWVudA==\r\n--x--\r\n";
        let parsed = parse_inbound(raw, 4096, 1024).unwrap();
        assert_eq!(parsed.sender.as_str(), "allowed@example.com");
        assert!(parsed.body.contains("reply text"));
        assert_eq!(parsed.attachments.len(), 1);
        assert!(!format!("{parsed:?}").contains("sentinel"));
        assert!(!serde_json::to_string(&parsed.attachments)
            .unwrap()
            .contains("sentinel"));
    }

    #[test]
    fn flags_automated_and_bulk_before_inference() {
        let raw = b"From: robot@example.com\r\nAuto-Submitted: auto-generated\r\nPrecedence: bulk\r\n\r\nbody";
        let parsed = parse_inbound(raw, 4096, 1024).unwrap();
        assert!(parsed.auto_submitted);
        assert!(parsed.list_or_bulk);
    }

    #[test]
    fn raw_and_decoded_bounds_fail_closed() {
        assert_eq!(
            parse_inbound(b"From: a@b.co\r\n\r\nbody", 4, 4),
            Err(MimeError::Bound)
        );
        assert_eq!(
            parse_inbound(b"From: a@b.co\r\n\r\n12345", 1024, 4),
            Err(MimeError::Bound)
        );
    }
}
