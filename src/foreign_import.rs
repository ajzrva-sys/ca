use crate::app::{Message, Role};
use crate::session::{ForeignTranscriptSource, ImportProvenance, SessionV2};
use crate::session_portability::ImportDefaults;
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const MAX_FOREIGN_FILES: usize = 1_000;
pub const MAX_FOREIGN_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FOREIGN_MESSAGES: usize = 100_000;
const MAX_MESSAGES_PER_SESSION: usize = 4_096;
const MAX_MESSAGE_BYTES: usize = 512 * 1024;
const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_ERRORS: usize = 32;
const MAX_ERROR_BYTES: usize = 256;
const MAX_SOURCE_LABEL_BYTES: usize = 128;
const MAX_TITLE_BYTES: usize = 1_024;
const MAX_DIRECTORY_DEPTH: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum ForeignImportError {
    #[error("{0}")]
    Invalid(String),
    #[error("source I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("OpenCode database is unavailable or invalid")]
    Sqlite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignImportPreview {
    pub source: ForeignTranscriptSource,
    pub source_label: String,
    pub source_files: usize,
    pub source_bytes: u64,
    pub sessions: usize,
    pub messages: usize,
    pub skipped_items: usize,
    pub errors: Vec<String>,
    pub learning_eligible: bool,
}

#[derive(Clone)]
pub struct ForeignImportBatch {
    pub preview: ForeignImportPreview,
    pub sessions: Vec<SessionV2>,
}

impl std::fmt::Debug for ForeignImportBatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ForeignImportBatch")
            .field("preview", &self.preview)
            .field("sessions", &self.sessions.len())
            .finish()
    }
}

#[derive(Default)]
struct ParseStats {
    skipped: usize,
    errors: Vec<String>,
}

impl ParseStats {
    fn skip(&mut self) {
        self.skipped = self.skipped.saturating_add(1);
    }

    fn error(&mut self, message: impl AsRef<str>) {
        self.skip();
        if self.errors.len() < MAX_ERRORS {
            self.errors
                .push(bounded_safe(message.as_ref(), MAX_ERROR_BYTES));
        }
    }
}

struct SourceSession {
    title: String,
    source_id: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    messages: Vec<Message>,
    skipped_items: usize,
    errors: Vec<String>,
}

impl ForeignTranscriptSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
            Self::OpenCode => "OpenCode",
        }
    }

    pub fn command_name(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::OpenCode => "opencode",
        }
    }
}

pub fn parse_source(
    path: &Path,
    source: ForeignTranscriptSource,
    defaults: ImportDefaults,
    learning_eligible: bool,
) -> Result<ForeignImportBatch, ForeignImportError> {
    let source_label = safe_source_label(path);
    let (files, source_bytes, traversal_skipped) = collect_source_files(path, source)?;
    let imported_at = Utc::now();
    let mut stats = ParseStats {
        skipped: traversal_skipped,
        errors: Vec::new(),
    };
    let mut source_sessions = Vec::new();

    for file in &files {
        let result = match source {
            ForeignTranscriptSource::Codex => parse_codex_file(file),
            ForeignTranscriptSource::Claude => parse_claude_file(file),
            ForeignTranscriptSource::OpenCode => parse_opencode_database(file),
        };
        match result {
            Ok(mut parsed) => {
                for session in &parsed {
                    for error in &session.errors {
                        if stats.errors.len() < MAX_ERRORS {
                            stats.errors.push(error.clone());
                        }
                    }
                }
                source_sessions.append(&mut parsed);
            }
            Err(error) => stats.error(format!("{}: {error}", safe_source_label(file))),
        }
    }

    if source_sessions.len() > MAX_FOREIGN_FILES {
        return Err(ForeignImportError::Invalid(format!(
            "foreign source contains more than {MAX_FOREIGN_FILES} sessions"
        )));
    }
    let total_messages = source_sessions
        .iter()
        .try_fold(0usize, |total, session| {
            total.checked_add(session.messages.len())
        })
        .ok_or_else(|| ForeignImportError::Invalid("foreign message count overflowed".into()))?;
    if total_messages > MAX_FOREIGN_MESSAGES {
        return Err(ForeignImportError::Invalid(format!(
            "foreign source contains more than {MAX_FOREIGN_MESSAGES} visible messages"
        )));
    }

    let sessions = source_sessions
        .into_iter()
        .map(|source_session| {
            let mut session = SessionV2::new(
                source_session.title,
                defaults.workspace.clone(),
                defaults.provider.clone(),
                defaults.model.clone(),
                source_session.messages,
            );
            session.reasoning_effort = defaults.reasoning_effort;
            session.created_at = source_session.created_at;
            session.updated_at = source_session.updated_at;
            session.import_provenance = Some(ImportProvenance {
                source,
                source_label: source_label.clone(),
                source_session_digest: digest_source_id(&source_session.source_id),
                message_count: session.messages.len(),
                skipped_items: source_session.skipped_items,
                learning_eligible,
                imported_at,
            });
            session
        })
        .collect::<Vec<_>>();
    let skipped_items = stats.skipped.saturating_add(
        sessions
            .iter()
            .filter_map(|session| session.import_provenance.as_ref())
            .map(|provenance| provenance.skipped_items)
            .sum::<usize>(),
    );
    Ok(ForeignImportBatch {
        preview: ForeignImportPreview {
            source,
            source_label,
            source_files: files.len(),
            source_bytes,
            sessions: sessions.len(),
            messages: total_messages,
            skipped_items,
            errors: stats.errors,
            learning_eligible,
        },
        sessions,
    })
}

fn collect_source_files(
    root: &Path,
    source: ForeignTranscriptSource,
) -> Result<(Vec<PathBuf>, u64, usize), ForeignImportError> {
    let metadata = std::fs::symlink_metadata(root).map_err(|error| {
        ForeignImportError::Invalid(format!("source cannot be inspected: {error}"))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(ForeignImportError::Invalid(
            "foreign import refuses symbolic-link sources".into(),
        ));
    }
    let mut skipped = 0usize;
    let mut files = Vec::new();
    if metadata.is_file() {
        if !is_source_file(root, source) {
            return Err(ForeignImportError::Invalid(format!(
                "{} import requires {}",
                source.label(),
                source_file_description(source)
            )));
        }
        files.push(root.to_path_buf());
    } else if metadata.is_dir() {
        let mut queue = VecDeque::from([(root.to_path_buf(), 0usize)]);
        while let Some((directory, depth)) = queue.pop_front() {
            if depth > MAX_DIRECTORY_DEPTH {
                return Err(ForeignImportError::Invalid(format!(
                    "foreign source exceeds the {MAX_DIRECTORY_DEPTH}-directory depth limit"
                )));
            }
            let mut entries = std::fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
            entries.sort_by_key(std::fs::DirEntry::file_name);
            for entry in entries {
                let path = entry.path();
                let metadata = std::fs::symlink_metadata(&path)?;
                if metadata.file_type().is_symlink() {
                    skipped = skipped.saturating_add(1);
                } else if metadata.is_dir() {
                    queue.push_back((path, depth + 1));
                } else if metadata.is_file() && is_source_file(&path, source) {
                    files.push(path);
                    if files.len() > MAX_FOREIGN_FILES {
                        return Err(ForeignImportError::Invalid(format!(
                            "foreign source contains more than {MAX_FOREIGN_FILES} source files"
                        )));
                    }
                } else {
                    skipped = skipped.saturating_add(1);
                }
            }
        }
    } else {
        return Err(ForeignImportError::Invalid(
            "foreign import source is not a regular file or directory".into(),
        ));
    }
    files.sort();
    if files.is_empty() {
        return Err(ForeignImportError::Invalid(format!(
            "foreign source contains no {}",
            source_file_description(source)
        )));
    }
    let mut bytes = 0u64;
    for file in &files {
        let metadata = std::fs::symlink_metadata(file)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(ForeignImportError::Invalid(
                "foreign source changed during discovery".into(),
            ));
        }
        bytes = bytes.checked_add(metadata.len()).ok_or_else(|| {
            ForeignImportError::Invalid("foreign source byte count overflowed".into())
        })?;
        if bytes > MAX_FOREIGN_BYTES {
            return Err(ForeignImportError::Invalid(
                "foreign source exceeds the 64 MiB input limit".into(),
            ));
        }
    }
    Ok((files, bytes, skipped))
}

fn is_source_file(path: &Path, source: ForeignTranscriptSource) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match source {
        ForeignTranscriptSource::Codex | ForeignTranscriptSource::Claude => extension == "jsonl",
        ForeignTranscriptSource::OpenCode => {
            matches!(extension.as_str(), "db" | "sqlite" | "sqlite3")
        }
    }
}

fn source_file_description(source: ForeignTranscriptSource) -> &'static str {
    match source {
        ForeignTranscriptSource::Codex | ForeignTranscriptSource::Claude => "JSONL files",
        ForeignTranscriptSource::OpenCode => "SQLite database files",
    }
}

fn read_bounded_file(path: &Path) -> Result<Vec<u8>, ForeignImportError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_FOREIGN_BYTES {
        return Err(ForeignImportError::Invalid(
            "foreign source is not a bounded regular file".into(),
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_FOREIGN_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FOREIGN_BYTES {
        return Err(ForeignImportError::Invalid(
            "foreign source exceeds the 64 MiB input limit".into(),
        ));
    }
    Ok(bytes)
}

fn parse_codex_file(path: &Path) -> Result<Vec<SourceSession>, ForeignImportError> {
    let bytes = read_bounded_file(path)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| ForeignImportError::Invalid("Codex JSONL is not UTF-8".into()))?;
    let fallback = Utc::now();
    let mut stats = ParseStats::default();
    let mut messages = Vec::new();
    let mut source_id = file_stem(path);
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        if line.len() > MAX_LINE_BYTES {
            stats.error("oversized JSONL record");
            continue;
        }
        let entry: Value = match serde_json::from_str(line) {
            Ok(entry) => entry,
            Err(_) => {
                stats.error("malformed JSONL record");
                continue;
            }
        };
        let entry_type = entry.get("type").and_then(Value::as_str);
        let payload = entry.get("payload").unwrap_or(&Value::Null);
        if entry_type == Some("session_meta") {
            if let Some(id) = payload.get("id").and_then(Value::as_str) {
                source_id = bounded_safe(id, MAX_SOURCE_LABEL_BYTES);
            }
            continue;
        }
        if entry_type != Some("response_item")
            || payload.get("type").and_then(Value::as_str) != Some("message")
        {
            stats.skip();
            continue;
        }
        let role = match payload.get("role").and_then(Value::as_str) {
            Some("user") => Role::User,
            Some("assistant") => Role::Assistant,
            _ => {
                stats.skip();
                continue;
            }
        };
        let content = extract_text_blocks(
            payload.get("content"),
            &role,
            SourceShape::Codex,
            &mut stats,
        );
        if content.is_empty() {
            stats.skip();
            continue;
        }
        let timestamp = parse_timestamp(entry.get("timestamp"), fallback);
        push_visible_message(&mut messages, role, content, timestamp, &mut stats)?;
    }
    finish_text_session("Codex session", source_id, messages, stats, fallback)
}

fn parse_claude_file(path: &Path) -> Result<Vec<SourceSession>, ForeignImportError> {
    let bytes = read_bounded_file(path)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| ForeignImportError::Invalid("Claude JSONL is not UTF-8".into()))?;
    let fallback = Utc::now();
    let mut stats = ParseStats::default();
    let mut messages = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        if line.len() > MAX_LINE_BYTES {
            stats.error("oversized JSONL record");
            continue;
        }
        let entry: Value = match serde_json::from_str(line) {
            Ok(entry) => entry,
            Err(_) => {
                stats.error("malformed JSONL record");
                continue;
            }
        };
        if entry.get("isMeta").and_then(Value::as_bool) == Some(true)
            || entry.get("isSidechain").and_then(Value::as_bool) == Some(true)
        {
            stats.skip();
            continue;
        }
        let role = match entry.get("type").and_then(Value::as_str) {
            Some("user") => Role::User,
            Some("assistant") => Role::Assistant,
            _ => {
                stats.skip();
                continue;
            }
        };
        let message = match entry.get("message") {
            Some(message) => message,
            None => {
                stats.skip();
                continue;
            }
        };
        let content = extract_text_blocks(
            message.get("content"),
            &role,
            SourceShape::Claude,
            &mut stats,
        );
        if content.is_empty() {
            stats.skip();
            continue;
        }
        let timestamp = parse_timestamp(entry.get("timestamp"), fallback);
        push_visible_message(&mut messages, role, content, timestamp, &mut stats)?;
    }
    finish_text_session(
        "Claude Code session",
        file_stem(path),
        messages,
        stats,
        fallback,
    )
}

#[derive(Clone, Copy)]
enum SourceShape {
    Codex,
    Claude,
}

fn extract_text_blocks(
    value: Option<&Value>,
    role: &Role,
    shape: SourceShape,
    stats: &mut ParseStats,
) -> String {
    if let Some(text) = value.and_then(Value::as_str) {
        return sanitize_visible_text(text);
    }
    let Some(blocks) = value.and_then(Value::as_array) else {
        return String::new();
    };
    let allowed = match (shape, role) {
        (SourceShape::Codex, Role::User) => &["input_text", "text"][..],
        (SourceShape::Codex, Role::Assistant) => &["output_text", "text"][..],
        (SourceShape::Claude, _) => &["text"][..],
        _ => &[][..],
    };
    let mut visible = Vec::new();
    for block in blocks {
        let block_type = block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if allowed.contains(&block_type) {
            if let Some(text) = block.get("text").and_then(Value::as_str) {
                let text = sanitize_visible_text(text);
                if !text.is_empty() {
                    visible.push(text);
                }
            }
        } else {
            stats.skip();
        }
    }
    visible.join("\n")
}

fn finish_text_session(
    default_title: &str,
    source_id: String,
    messages: Vec<Message>,
    stats: ParseStats,
    fallback: DateTime<Utc>,
) -> Result<Vec<SourceSession>, ForeignImportError> {
    if messages.is_empty() {
        return Err(ForeignImportError::Invalid(
            "source contains no visible user or assistant messages".into(),
        ));
    }
    let title = title_from_messages(&messages).unwrap_or_else(|| default_title.into());
    let created_at = messages
        .iter()
        .map(|message| message.timestamp)
        .min()
        .unwrap_or(fallback);
    let updated_at = messages
        .iter()
        .map(|message| message.timestamp)
        .max()
        .unwrap_or(created_at);
    Ok(vec![SourceSession {
        title,
        source_id,
        created_at,
        updated_at,
        messages,
        skipped_items: stats.skipped,
        errors: stats.errors,
    }])
}

fn parse_opencode_database(path: &Path) -> Result<Vec<SourceSession>, ForeignImportError> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_FOREIGN_BYTES
    {
        return Err(ForeignImportError::Invalid(
            "OpenCode source is not a bounded regular database".into(),
        ));
    }
    let mut uri = url::Url::from_file_path(path)
        .map_err(|_| ForeignImportError::Invalid("OpenCode database path is invalid".into()))?;
    uri.query_pairs_mut()
        .append_pair("mode", "ro")
        .append_pair("immutable", "1");
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection =
        Connection::open_with_flags(uri.as_str(), flags).map_err(|_| ForeignImportError::Sqlite)?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(|_| ForeignImportError::Sqlite)?;
    for table in ["session", "message", "part"] {
        let exists = connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |_| Ok(()),
            )
            .optional()
            .map_err(|_| ForeignImportError::Sqlite)?
            .is_some();
        if !exists {
            return Err(ForeignImportError::Invalid(format!(
                "OpenCode database is missing the {table} table"
            )));
        }
    }

    let mut statement = connection
        .prepare(
            "SELECT id, title, time_created, time_updated FROM session \
             ORDER BY time_created, id LIMIT ?1",
        )
        .map_err(|_| ForeignImportError::Sqlite)?;
    let rows = statement
        .query_map([MAX_FOREIGN_FILES as i64 + 1], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|_| ForeignImportError::Sqlite)?;
    let records = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ForeignImportError::Sqlite)?;
    if records.len() > MAX_FOREIGN_FILES {
        return Err(ForeignImportError::Invalid(format!(
            "OpenCode database contains more than {MAX_FOREIGN_FILES} sessions"
        )));
    }
    let fallback = Utc::now();
    let mut sessions = Vec::new();
    for (source_id, stored_title, created_ms, updated_ms) in records {
        let mut stats = ParseStats::default();
        let mut messages = Vec::new();
        let mut message_statement = connection
            .prepare(
                "SELECT id, time_created, data FROM message \
                 WHERE session_id = ?1 ORDER BY time_created, id LIMIT ?2",
            )
            .map_err(|_| ForeignImportError::Sqlite)?;
        let message_rows = message_statement
            .query_map(
                rusqlite::params![&source_id, MAX_MESSAGES_PER_SESSION as i64 + 1],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .map_err(|_| ForeignImportError::Sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ForeignImportError::Sqlite)?;
        if message_rows.len() > MAX_MESSAGES_PER_SESSION {
            stats.error("session exceeds the visible message limit");
            continue;
        }
        for (message_id, created, data) in message_rows {
            let data: Value = match serde_json::from_str(&data) {
                Ok(data) => data,
                Err(_) => {
                    stats.error("malformed OpenCode message record");
                    continue;
                }
            };
            let role = match data.get("role").and_then(Value::as_str) {
                Some("user") => Role::User,
                Some("assistant") => Role::Assistant,
                _ => {
                    stats.skip();
                    continue;
                }
            };
            let mut part_statement = connection
                .prepare(
                    "SELECT data FROM part WHERE session_id = ?1 AND message_id = ?2 \
                     ORDER BY time_created, id",
                )
                .map_err(|_| ForeignImportError::Sqlite)?;
            let part_rows = part_statement
                .query_map(rusqlite::params![&source_id, &message_id], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|_| ForeignImportError::Sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| ForeignImportError::Sqlite)?;
            let mut visible = Vec::new();
            for part in part_rows {
                let part: Value = match serde_json::from_str(&part) {
                    Ok(part) => part,
                    Err(_) => {
                        stats.error("malformed OpenCode part record");
                        continue;
                    }
                };
                if part.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        let text = sanitize_visible_text(text);
                        if !text.is_empty() {
                            visible.push(text);
                        }
                    }
                } else {
                    stats.skip();
                }
            }
            if visible.is_empty() {
                stats.skip();
                continue;
            }
            push_visible_message(
                &mut messages,
                role,
                visible.join("\n"),
                timestamp_from_millis(created, fallback),
                &mut stats,
            )?;
        }
        if messages.is_empty() {
            continue;
        }
        let title = sanitize_title(&stored_title)
            .or_else(|| title_from_messages(&messages))
            .unwrap_or_else(|| "OpenCode session".into());
        let created_at = timestamp_from_millis(created_ms, fallback);
        let updated_at = timestamp_from_millis(updated_ms, fallback).max(created_at);
        sessions.push(SourceSession {
            title,
            source_id,
            created_at,
            updated_at,
            messages,
            skipped_items: stats.skipped,
            errors: stats.errors,
        });
    }
    Ok(sessions)
}

fn push_visible_message(
    messages: &mut Vec<Message>,
    role: Role,
    content: String,
    timestamp: DateTime<Utc>,
    stats: &mut ParseStats,
) -> Result<(), ForeignImportError> {
    if messages.len() >= MAX_MESSAGES_PER_SESSION {
        return Err(ForeignImportError::Invalid(format!(
            "foreign session exceeds {MAX_MESSAGES_PER_SESSION} visible messages"
        )));
    }
    if content.len() > MAX_MESSAGE_BYTES {
        stats.error("visible message exceeds the 512 KiB limit");
        return Ok(());
    }
    if content.trim().is_empty() {
        stats.skip();
        return Ok(());
    }
    messages.push(Message {
        role,
        content,
        tool_calls: None,
        thinking: None,
        timestamp,
        response_items: Vec::new(),
    });
    Ok(())
}

fn parse_timestamp(value: Option<&Value>, fallback: DateTime<Utc>) -> DateTime<Utc> {
    match value {
        Some(Value::String(value)) => DateTime::parse_from_rfc3339(value)
            .map(|value| value.with_timezone(&Utc))
            .unwrap_or(fallback),
        Some(Value::Number(value)) => value
            .as_i64()
            .map(|value| timestamp_from_millis(value, fallback))
            .unwrap_or(fallback),
        _ => fallback,
    }
}

fn timestamp_from_millis(value: i64, fallback: DateTime<Utc>) -> DateTime<Utc> {
    let value = if value.unsigned_abs() < 10_000_000_000 {
        value.saturating_mul(1_000)
    } else {
        value
    };
    Utc.timestamp_millis_opt(value).single().unwrap_or(fallback)
}

fn title_from_messages(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .find(|message| message.role == Role::User)
        .and_then(|message| sanitize_title(&message.content))
}

fn sanitize_title(value: &str) -> Option<String> {
    let title = sanitize_visible_text(value)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if title.is_empty() {
        None
    } else {
        Some(bounded_safe(&title, MAX_TITLE_BYTES))
    }
}

fn sanitize_visible_text(value: &str) -> String {
    let mut output = Vec::new();
    for line in value.replace("\r\n", "\n").lines() {
        let lower = line.to_ascii_lowercase();
        if [
            "authorization: bearer",
            "api_key=",
            "api-key:",
            "password=",
            "password:",
            "secret=",
            "private key",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
        {
            output.push("[Credential-like content omitted]".to_string());
            continue;
        }
        if [
            "data:image/",
            "data:audio/",
            "data:video/",
            "data:application/octet-stream",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
        {
            output.push("[Binary/media payload omitted]".to_string());
            continue;
        }
        let safe = crate::terminal_text::sanitize_terminal_text(line);
        output.push(redact_secret_tokens(&safe));
    }
    output.join("\n").trim().to_string()
}

fn redact_secret_tokens(line: &str) -> String {
    line.split_inclusive(char::is_whitespace)
        .map(|token| {
            let bare = token.trim_end_matches(char::is_whitespace);
            let suffix = &token[bare.len()..];
            let candidate = bare.trim_matches(|character: char| {
                matches!(
                    character,
                    '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
                )
            });
            if (candidate.starts_with("sk-")
                || candidate.starts_with("ghp_")
                || candidate.starts_with("xoxb-")
                || candidate.starts_with("AKIA"))
                && candidate.len() >= 12
            {
                format!("[REDACTED]{suffix}")
            } else {
                token.to_string()
            }
        })
        .collect()
}

fn safe_source_label(path: &Path) -> String {
    let label = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("selected source");
    bounded_safe(label, MAX_SOURCE_LABEL_BYTES)
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(|value| bounded_safe(value, MAX_SOURCE_LABEL_BYTES))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "foreign-session".into())
}

fn digest_source_id(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn bounded_safe(value: &str, max_bytes: usize) -> String {
    let safe = crate::terminal_text::sanitize_terminal_text(value);
    if safe.len() <= max_bytes {
        return safe;
    }
    let mut end = max_bytes;
    while end > 0 && !safe.is_char_boundary(end) {
        end -= 1;
    }
    safe[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn defaults(root: &Path) -> ImportDefaults {
        ImportDefaults {
            workspace: root.to_string_lossy().into_owned(),
            provider: "safe".into(),
            model: "current".into(),
            reasoning_effort: None,
        }
    }

    #[test]
    fn codex_import_keeps_only_visible_text_and_safe_provenance() {
        let root = tempdir().unwrap();
        let path = root.path().join("codex.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"foreign-id\",\"cwd\":\"/private/source\"}}\n",
                "{\"type\":\"response_item\",\"timestamp\":\"2026-08-01T00:00:00Z\",\"payload\":{\"type\":\"message\",\"role\":\"developer\",\"content\":[{\"type\":\"input_text\",\"text\":\"hidden system\"}]}}\n",
                "{\"type\":\"response_item\",\"timestamp\":\"2026-08-01T00:00:01Z\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"hello sk-123456789012345\"},{\"type\":\"input_image\",\"image_url\":\"secret\"}]}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"reasoning\",\"summary\":[{\"text\":\"hidden thought\"}]}}\n",
                "{\"type\":\"response_item\",\"timestamp\":\"2026-08-01T00:00:02Z\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"visible answer\"}]}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call\",\"name\":\"shell\",\"arguments\":\"secret output\"}}\n"
            ),
        )
        .unwrap();
        let batch = parse_source(
            &path,
            ForeignTranscriptSource::Codex,
            defaults(root.path()),
            false,
        )
        .unwrap();
        assert_eq!(batch.preview.sessions, 1);
        assert_eq!(batch.preview.messages, 2);
        let imported = &batch.sessions[0];
        assert_eq!(imported.provider, "safe");
        assert_eq!(imported.model, "current");
        assert_eq!(imported.messages[0].content, "hello [REDACTED]");
        let serialized = serde_json::to_string(imported).unwrap();
        for forbidden in [
            "hidden system",
            "hidden thought",
            "secret output",
            "/private/source",
            "foreign-id",
        ] {
            assert!(!serialized.contains(forbidden), "found {forbidden}");
        }
        let provenance = imported.import_provenance.as_ref().unwrap();
        assert!(!provenance.learning_eligible);
        assert_eq!(provenance.source_label, "codex.jsonl");
        assert_eq!(provenance.source_session_digest.len(), 64);
    }

    #[test]
    fn claude_import_omits_thinking_tools_images_meta_and_sidechains() {
        let root = tempdir().unwrap();
        let path = root.path().join("claude.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"system\",\"message\":{\"content\":\"system secret\"}}\n",
                "{\"type\":\"user\",\"timestamp\":\"2026-08-02T00:00:00Z\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"question\"},{\"type\":\"image\",\"source\":{\"data\":\"media-secret\"}},{\"type\":\"tool_result\",\"content\":\"tool-secret\"}]}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-08-02T00:00:01Z\",\"message\":{\"content\":[{\"type\":\"thinking\",\"thinking\":\"thought-secret\"},{\"type\":\"tool_use\",\"input\":{\"secret\":\"tool-input\"}},{\"type\":\"text\",\"text\":\"answer\"}]}}\n",
                "{\"type\":\"assistant\",\"isSidechain\":true,\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"branch-secret\"}]}}\n"
            ),
        )
        .unwrap();
        let batch = parse_source(
            &path,
            ForeignTranscriptSource::Claude,
            defaults(root.path()),
            true,
        )
        .unwrap();
        assert_eq!(batch.preview.messages, 2);
        let serialized = serde_json::to_string(&batch.sessions).unwrap();
        assert!(serialized.contains("question"));
        assert!(serialized.contains("answer"));
        assert!(
            batch.sessions[0]
                .import_provenance
                .as_ref()
                .unwrap()
                .learning_eligible
        );
        for forbidden in [
            "system secret",
            "media-secret",
            "tool-secret",
            "thought-secret",
            "tool-input",
            "branch-secret",
        ] {
            assert!(!serialized.contains(forbidden), "found {forbidden}");
        }
    }

    #[test]
    fn directory_scan_is_explicit_bounded_and_never_follows_symlinks() {
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(
            source.join("one.jsonl"),
            "{\"type\":\"user\",\"message\":{\"content\":\"hello\"}}\n",
        )
        .unwrap();
        fs::write(source.join("ignored.txt"), "not history").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.path().join("outside.jsonl"), source.join("link.jsonl"))
            .unwrap();
        let batch = parse_source(
            &source,
            ForeignTranscriptSource::Claude,
            defaults(root.path()),
            false,
        )
        .unwrap();
        assert_eq!(batch.preview.source_files, 1);
        assert!(batch.preview.skipped_items >= 1);

        #[cfg(unix)]
        {
            let direct = root.path().join("direct.jsonl");
            fs::write(&direct, "{}").unwrap();
            let link = root.path().join("direct-link.jsonl");
            std::os::unix::fs::symlink(&direct, &link).unwrap();
            assert!(parse_source(
                &link,
                ForeignTranscriptSource::Claude,
                defaults(root.path()),
                false,
            )
            .unwrap_err()
            .to_string()
            .contains("symbolic-link"));
        }
    }

    #[test]
    fn opencode_database_is_read_immutable_and_imports_only_text_parts() {
        let root = tempdir().unwrap();
        let path = root.path().join("opencode.db");
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE session (id TEXT PRIMARY KEY, title TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL);\
                     CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, data TEXT NOT NULL);\
                     CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT NOT NULL, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, data TEXT NOT NULL);\
                     INSERT INTO session VALUES ('source-session', 'Imported title', 1000, 3000);\
                     INSERT INTO message VALUES ('m1', 'source-session', 1000, '{\"role\":\"user\"}');\
                     INSERT INTO message VALUES ('m2', 'source-session', 2000, '{\"role\":\"assistant\"}');\
                     INSERT INTO part VALUES ('p1', 'm1', 'source-session', 1000, '{\"type\":\"text\",\"text\":\"hello\"}');\
                     INSERT INTO part VALUES ('p2', 'm2', 'source-session', 2000, '{\"type\":\"reasoning\",\"text\":\"hidden\"}');\
                     INSERT INTO part VALUES ('p3', 'm2', 'source-session', 2001, '{\"type\":\"tool\",\"output\":\"tool-secret\"}');\
                     INSERT INTO part VALUES ('p4', 'm2', 'source-session', 2002, '{\"type\":\"text\",\"text\":\"answer\"}');",
                )
                .unwrap();
        }
        let before = fs::read(&path).unwrap();
        let batch = parse_source(
            &path,
            ForeignTranscriptSource::OpenCode,
            defaults(root.path()),
            false,
        )
        .unwrap();
        assert_eq!(batch.preview.sessions, 1);
        assert_eq!(batch.preview.messages, 2);
        let serialized = serde_json::to_string(&batch.sessions).unwrap();
        assert!(serialized.contains("hello"));
        assert!(serialized.contains("answer"));
        assert!(!serialized.contains("hidden"));
        assert!(!serialized.contains("tool-secret"));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(!root.path().join("opencode.db-wal").exists());
        assert!(!root.path().join("opencode.db-shm").exists());
    }

    #[test]
    fn legacy_sessions_without_import_provenance_remain_compatible() {
        let session = SessionV2::new("Legacy", "/workspace", "safe", "model", Vec::new());
        let mut value = serde_json::to_value(&session).unwrap();
        value.as_object_mut().unwrap().remove("import_provenance");
        let loaded: SessionV2 = serde_json::from_value(value).unwrap();
        assert!(loaded.import_provenance.is_none());
    }

    #[test]
    fn source_file_and_total_byte_caps_fail_before_parsing() {
        let root = tempdir().unwrap();
        let many = root.path().join("many");
        fs::create_dir(&many).unwrap();
        for index in 0..=MAX_FOREIGN_FILES {
            fs::write(many.join(format!("{index:04}.jsonl")), "").unwrap();
        }
        assert!(parse_source(
            &many,
            ForeignTranscriptSource::Codex,
            defaults(root.path()),
            false,
        )
        .unwrap_err()
        .to_string()
        .contains("source files"));

        let oversized = root.path().join("oversized.jsonl");
        let file = fs::File::create(&oversized).unwrap();
        file.set_len(MAX_FOREIGN_BYTES + 1).unwrap();
        assert!(parse_source(
            &oversized,
            ForeignTranscriptSource::Claude,
            defaults(root.path()),
            false,
        )
        .unwrap_err()
        .to_string()
        .contains("64 MiB"));
    }

    #[test]
    fn partial_directory_failures_remain_visible_without_losing_good_sessions() {
        let root = tempdir().unwrap();
        let source = root.path().join("codex");
        fs::create_dir(&source).unwrap();
        fs::write(
            source.join("good.jsonl"),
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"good\"}]}}\n",
        )
        .unwrap();
        fs::write(source.join("bad.jsonl"), "not-json\n").unwrap();
        let batch = parse_source(
            &source,
            ForeignTranscriptSource::Codex,
            defaults(root.path()),
            false,
        )
        .unwrap();
        assert_eq!(batch.preview.sessions, 1);
        assert_eq!(batch.preview.messages, 1);
        assert_eq!(batch.preview.errors.len(), 1);
        assert!(batch.preview.skipped_items >= 1);
    }

    #[test]
    fn session_store_rejects_unbounded_or_path_bearing_provenance() {
        let root = tempdir().unwrap();
        let store = crate::session::SessionStore::new(root.path().join("sessions"));
        let mut session = SessionV2::new(
            "Imported",
            root.path().to_string_lossy(),
            "safe",
            "model",
            Vec::new(),
        );
        session.import_provenance = Some(ImportProvenance {
            source: ForeignTranscriptSource::Claude,
            source_label: "/private/source.jsonl".into(),
            source_session_digest: "a".repeat(64),
            message_count: 0,
            skipped_items: 0,
            learning_eligible: false,
            imported_at: Utc::now(),
        });
        assert!(store
            .save(&mut session)
            .unwrap_err()
            .to_string()
            .contains("provenance"));
        assert!(store.list().unwrap().is_empty());
    }
}
