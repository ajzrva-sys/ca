//! Worker retry briefs and failure fingerprints.
//!
//! Owns compact, machine-auditable retry packets (`WorkerRetryBrief`),
//! compiler-diagnostic extraction, and failure fingerprint classification used
//! to decide mechanical retry vs Senior/stop routes.
//!
//! Read when changing retry fingerprint kinds or repair-instruction generation.
//! Do not read for Markdown queue mutation or git commits (those stay in `app`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct WorkerRetryBrief {
    // This is the machine-readable replacement for "try again, I guess."  It is
    // compact enough to fit in the Worker prompt and precise enough for Rust to
    // audit retry loops.
    pub(crate) task_id: String,
    pub(crate) attempt: u32,
    pub(crate) route: String,
    pub(crate) worker_status: String,
    pub(crate) blocker: String,
    pub(crate) changed_files: Vec<String>,
    pub(crate) diff_stat: String,
    pub(crate) diagnostic_command: Option<String>,
    pub(crate) diagnostic_category: Option<String>,
    pub(crate) diagnostic_summary: Option<String>,
    pub(crate) fingerprint: Option<FailureFingerprint>,
    pub(crate) fingerprint_retry_count: u32,
    #[serde(default)]
    pub(crate) top_compiler_errors: Vec<CompilerDiagnostic>,
    #[serde(default)]
    pub(crate) patch_context_mismatch_count: u32,
    #[serde(default)]
    pub(crate) patch_context_hint: Option<String>,
    // Human/model facing instruction generated from the fingerprint.  The raw
    // fingerprint fields above remain the durable authority.
    pub(crate) repair_instruction: String,
    #[serde(default)]
    pub(crate) verification_improved: bool,
    #[serde(default)]
    pub(crate) prior_content_hashes: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default)]
pub(crate) struct CompilerDiagnostic {
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
    pub(crate) column: Option<u32>,
    pub(crate) message: String,
}

impl CompilerDiagnostic {
    pub(crate) fn display(&self) -> String {
        match (&self.file, self.line, self.column) {
            (Some(file), Some(line), Some(column)) => {
                format!("{file}:{line}:{column}: {}", self.message)
            }
            (Some(file), Some(line), None) => format!("{file}:{line}: {}", self.message),
            (Some(file), None, _) => format!("{file}: {}", self.message),
            _ => self.message.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct FailureFingerprint {
    // Fingerprints intentionally classify broad failure families, not exact
    // compiler diagnostics.  They are routing hints: mechanical -> retry,
    // environment -> record/stop, policy -> planner/senior.
    pub(crate) kind: FailureFingerprintKind,
    #[serde(default)]
    pub(crate) file: Option<String>,
    #[serde(default)]
    pub(crate) line: Option<u32>,
    #[serde(default)]
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) command: Option<String>,
}

impl FailureFingerprint {
    pub(crate) fn key(&self) -> String {
        // The key is stable enough for retry accounting but still specific to
        // file/line/message so one task can progress through distinct failures.
        format!(
            "{}|{}|{}|{}",
            self.kind.as_str(),
            self.file.as_deref().unwrap_or(""),
            self.line.map(|line| line.to_string()).unwrap_or_default(),
            self.message
        )
    }

    pub(crate) fn is_mechanical_retryable(&self) -> bool {
        // False negatives are okay: unknown failures can escalate.  False
        // positives are not, so only clearly local/mechanical cases retry.
        matches!(
            self.kind,
            FailureFingerprintKind::SwiftInitializerLabelMismatch
                | FailureFingerprintKind::SwiftCompileError
                | FailureFingerprintKind::TestAssertionFailure
                | FailureFingerprintKind::ApplyPatchContextMismatch
                | FailureFingerprintKind::RustTypeMismatch
                | FailureFingerprintKind::MissingImport
        )
    }

    pub(crate) fn is_environment_or_terminal(&self) -> bool {
        matches!(
            self.kind,
            FailureFingerprintKind::SwiftPmLockContention
                | FailureFingerprintKind::KnownEnvironmentBlocker
                | FailureFingerprintKind::VerificationTimeout
                | FailureFingerprintKind::RipgrepNoMatches
        )
    }

    pub(crate) fn is_policy_scope(&self) -> bool {
        self.kind == FailureFingerprintKind::PolicyScopeViolation
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FailureFingerprintKind {
    SwiftInitializerLabelMismatch,
    SwiftCompileError,
    TestAssertionFailure,
    SwiftPmLockContention,
    ApplyPatchContextMismatch,
    RustTypeMismatch,
    MissingImport,
    RipgrepNoMatches,
    KnownEnvironmentBlocker,
    VerificationTimeout,
    PolicyScopeViolation,
    Unknown,
}

impl FailureFingerprintKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::SwiftInitializerLabelMismatch => "swift_initializer_label_mismatch",
            Self::SwiftCompileError => "swift_compile_error",
            Self::TestAssertionFailure => "test_assertion_failure",
            Self::SwiftPmLockContention => "swiftpm_lock_contention",
            Self::ApplyPatchContextMismatch => "apply_patch_context_mismatch",
            Self::RustTypeMismatch => "rust_type_mismatch",
            Self::MissingImport => "missing_import",
            Self::RipgrepNoMatches => "rg_no_matches",
            Self::KnownEnvironmentBlocker => "known_environment_blocker",
            Self::VerificationTimeout => "verification_timeout",
            Self::PolicyScopeViolation => "policy_scope_violation",
            Self::Unknown => "unknown",
        }
    }
}

pub(crate) fn failure_fingerprint_from_text(
    command: Option<&str>,
    category: Option<&str>,
    text: &str,
) -> FailureFingerprint {
    // Heuristic by necessity, conservative by policy.  These patterns only
    // identify broad routing families; they do not attempt a full compiler parse.
    let lower = text.to_ascii_lowercase();
    let command_lower = command.unwrap_or("").to_ascii_lowercase();
    let compiler_errors = compiler_errors_from_text(text);
    let compiler_error_text = compiler_errors
        .iter()
        .map(|error| error.message.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    let (file, line) = extract_file_line(text);
    let message = compiler_errors
        .first()
        .map(CompilerDiagnostic::display)
        .or_else(|| first_useful_output_line(text))
        .unwrap_or_else(|| truncate_inline_local(text.trim(), 240))
        .trim()
        .to_string();
    let has_swift_compiler_error = compiler_errors.iter().any(|error| {
        error
            .file
            .as_deref()
            .is_some_and(|file| file.ends_with(".swift"))
    }) || command_lower.contains("swift")
        && (lower.contains(".swift:") || lower.contains("error:"));
    let kind = if category == Some("noMatches")
        || (command_lower.starts_with("rg ") && lower.contains("no matches"))
    {
        FailureFingerprintKind::RipgrepNoMatches
    } else if category == Some("knownEnvironmentBlocker")
        || lower.contains("sandbox_apply")
        || lower.contains("operation not permitted")
    {
        FailureFingerprintKind::KnownEnvironmentBlocker
    } else if category == Some("timedOut") || lower.contains("timed out") {
        FailureFingerprintKind::VerificationTimeout
    } else if lower.contains("another instance of swiftpm")
        || (lower.contains("swiftpm") && lower.contains(".build") && lower.contains("waiting"))
    {
        FailureFingerprintKind::SwiftPmLockContention
    } else if lower.contains("apply_patch context mismatch")
        || lower.contains("patch rejected")
        || lower.contains("tool note: apply_patch")
    {
        FailureFingerprintKind::ApplyPatchContextMismatch
    } else if has_swift_compiler_error
        && contains_swift_initializer_or_callsite_label_signal(&compiler_error_text, &lower)
    {
        FailureFingerprintKind::SwiftInitializerLabelMismatch
    } else if lower.contains("xctassert")
        || (lower.contains("test case") && lower.contains("failed"))
        || lower.contains("failing test")
    {
        FailureFingerprintKind::TestAssertionFailure
    } else if lower.contains("cannot find type")
        || lower.contains("cannot find value")
        || lower.contains("cannot find '")
        || lower.contains("no such module")
        || lower.contains("missing import")
    {
        FailureFingerprintKind::MissingImport
    } else if lower.contains("mismatched types")
        || lower.contains("type mismatch")
        || lower.contains("cannot convert value")
    {
        FailureFingerprintKind::RustTypeMismatch
    } else if has_swift_compiler_error {
        FailureFingerprintKind::SwiftCompileError
    } else if lower.contains("outside allowed")
        || lower.contains("outside the approved scope")
        || lower.contains("guard violation")
        || lower.contains("unapproved changes")
        || lower.contains("unapproved work")
        || lower.contains("approval/safety gate")
        || lower.contains("approval gate")
        || lower.contains("safety gate")
        || lower.contains("new approval")
        || lower.contains("requires explicit approval")
        || lower.contains("required new gating")
        || lower.contains("new gating requirement")
        || lower.contains("outside approved scope")
    {
        FailureFingerprintKind::PolicyScopeViolation
    } else {
        FailureFingerprintKind::Unknown
    };
    FailureFingerprint {
        kind,
        file,
        line,
        message,
        command: command.map(str::to_string),
    }
}

fn contains_swift_initializer_or_callsite_label_signal(error_text: &str, full_text: &str) -> bool {
    [
        "missing argument label",
        "extra argument",
        "extra arguments at positions",
        "incorrect argument label",
        "incorrect argument labels",
        "argument-label",
        "argument label",
        "invalid redeclaration",
    ]
    .iter()
    .any(|needle| error_text.contains(needle) || full_text.contains(needle))
        || (error_text.contains("initializer") && error_text.contains("argument"))
        || (full_text.contains("initializer") && full_text.contains("argument"))
}

pub(crate) fn compiler_errors_from_text(text: &str) -> Vec<CompilerDiagnostic> {
    let mut errors = Vec::new();
    for line in text.lines() {
        if let Some(error) = parse_compiler_error_line(line) {
            errors.push(error);
        }
        if errors.len() >= 3 {
            break;
        }
    }
    errors
}

fn parse_compiler_error_line(line: &str) -> Option<CompilerDiagnostic> {
    let trimmed = line.trim();
    let marker = ": error:";
    let marker_index = trimmed.find(marker)?;
    let prefix = &trimmed[..marker_index];
    let message = trimmed[marker_index + marker.len()..].trim();
    if message.is_empty() {
        return None;
    }
    let (file, line_number, column) = parse_file_line_column(prefix)?;
    Some(CompilerDiagnostic {
        file: Some(file),
        line: Some(line_number),
        column,
        message: message.to_string(),
    })
}

fn parse_file_line_column(prefix: &str) -> Option<(String, u32, Option<u32>)> {
    let mut parts = prefix.rsplitn(3, ':').collect::<Vec<_>>();
    if parts.len() < 2 {
        return None;
    }
    let last = parts[0];
    let second = parts[1];
    if let (Ok(column), Ok(line)) = (last.parse::<u32>(), second.parse::<u32>()) {
        let file = parts.get(2)?.trim();
        if looks_like_source_path(file) {
            return Some((file.to_string(), line, Some(column)));
        }
    }
    parts = prefix.rsplitn(2, ':').collect::<Vec<_>>();
    let line = parts.first()?.parse::<u32>().ok()?;
    let file = parts.get(1)?.trim();
    looks_like_source_path(file).then_some((file.to_string(), line, None))
}

pub(crate) fn patch_context_mismatch_count(text: &str) -> u32 {
    let lower = text.to_ascii_lowercase();
    let mut count = 0u32;
    for needle in [
        "apply_patch context mismatch",
        "tool note: apply_patch context mismatch",
        "patch rejected",
    ] {
        count = count.saturating_add(lower.matches(needle).count() as u32);
    }
    count
}

pub(crate) fn first_useful_output_line(text: &str) -> Option<String> {
    // Extract the line a human would quote in a repair brief.  Keep this small;
    // raw logs stay on disk in evidence files.
    let lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    for needle in [
        "error:",
        "fatal:",
        "failed",
        "failure",
        "xctassert",
        "cannot find",
        "argument",
        "mismatch",
    ] {
        if let Some(line) = lines
            .iter()
            .find(|line| line.to_ascii_lowercase().contains(needle))
        {
            return Some((*line).to_string());
        }
    }
    lines.first().map(|line| (*line).to_string())
}

pub(crate) fn extract_file_line(text: &str) -> (Option<String>, Option<u32>) {
    // Best-effort location extraction used in retry briefs.  Failure to find a
    // line should not block routing; the repair instruction can still be scoped.
    if let Some(error) = compiler_errors_from_text(text).into_iter().next() {
        return (error.file, error.line);
    }
    for line in text.lines() {
        if let Some((file, line_number)) = parse_colon_file_line(line) {
            return (Some(file), Some(line_number));
        }
    }
    (None, None)
}

fn parse_colon_file_line(line: &str) -> Option<(String, u32)> {
    let bytes = line.as_bytes();
    for idx in 1..bytes.len().saturating_sub(1) {
        if bytes[idx] != b':' {
            continue;
        }
        let rest = &line[idx + 1..];
        let digits_len = rest.chars().take_while(|ch| ch.is_ascii_digit()).count();
        if digits_len == 0 {
            continue;
        }
        let after_digits = rest.chars().nth(digits_len);
        if after_digits != Some(':') {
            continue;
        }
        let file = line[..idx].trim();
        if !looks_like_source_path(file) {
            continue;
        }
        let Ok(line_number) = rest[..digits_len].parse::<u32>() else {
            continue;
        };
        return Some((file.to_string(), line_number));
    }
    None
}

fn looks_like_source_path(file: &str) -> bool {
    !file.is_empty()
        && (file.ends_with(".swift")
            || file.ends_with(".rs")
            || file.ends_with(".md")
            || file.contains('/'))
}

fn truncate_inline_local(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out = s
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
}
