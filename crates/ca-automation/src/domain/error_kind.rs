//! Step-level error taxonomy for conductor orchestration.
//!
//! Canonical replacement for the ad-hoc `classify_step_error_kind` string-match
//! helper in `app.rs`. Variants map 1-to-1 with the historical string labels
//! so callers can migrate incrementally.

use std::fmt;

/// Top-level failure kind for a conductor step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepErrorKind {
    /// LLM response was unparseable or structurally invalid.
    MalformedOutput,
    /// Network-level failure reaching the provider.
    ProviderUnreachable,
    /// Provider is explicitly disabled in config.
    ProviderDisabled,
    /// A shared lock / mutex was contended or poisoned.
    LockContention,
    /// The step exceeded its deadline.
    Timeout,
    /// Catch-all for anything that does not match the above.
    Unknown,
}

impl StepErrorKind {
    /// Stable lowercase tag suitable for metrics / logging labels.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MalformedOutput => "malformed_output",
            Self::ProviderUnreachable => "provider_unreachable",
            Self::ProviderDisabled => "provider_disabled",
            Self::LockContention => "lock_contention",
            Self::Timeout => "timeout",
            Self::Unknown => "error",
        }
    }

    /// Classify a step-error summary string into a kind.
    ///
    /// Mirrors the keyword-match logic that previously lived in
    /// `classify_step_error_kind` inside `app.rs`.
    pub fn classify(summary: &str) -> Self {
        let lower = summary.to_ascii_lowercase();
        if lower.contains("failed to parse") || lower.contains("malformed") {
            Self::MalformedOutput
        } else if lower.contains("connection refused") || lower.contains("unreachable") {
            Self::ProviderUnreachable
        } else if lower.contains("enabled is false") {
            Self::ProviderDisabled
        } else if lower.contains("lock") {
            Self::LockContention
        } else if lower.contains("timed out") || lower.contains("timeout") {
            Self::Timeout
        } else {
            Self::Unknown
        }
    }
}

impl fmt::Display for StepErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::StepErrorKind;

    #[test]
    fn classify_malformed_output() {
        assert_eq!(
            StepErrorKind::classify("failed to parse JSON from model output"),
            StepErrorKind::MalformedOutput,
        );
        assert_eq!(
            StepErrorKind::classify("Response contained malformed YAML"),
            StepErrorKind::MalformedOutput,
        );
    }

    #[test]
    fn classify_provider_unreachable() {
        assert_eq!(
            StepErrorKind::classify("connection refused on port 11434"),
            StepErrorKind::ProviderUnreachable,
        );
        assert_eq!(
            StepErrorKind::classify("Host unreachable: ollama.local"),
            StepErrorKind::ProviderUnreachable,
        );
    }

    #[test]
    fn classify_provider_disabled() {
        assert_eq!(
            StepErrorKind::classify("Provider enabled is false in config"),
            StepErrorKind::ProviderDisabled,
        );
    }

    #[test]
    fn classify_lock_contention() {
        assert_eq!(
            StepErrorKind::classify("Could not acquire lock on shared state"),
            StepErrorKind::LockContention,
        );
    }

    #[test]
    fn classify_timeout() {
        assert_eq!(
            StepErrorKind::classify("Step timed out after 30s"),
            StepErrorKind::Timeout,
        );
        assert_eq!(
            StepErrorKind::classify("Request timeout waiting for response"),
            StepErrorKind::Timeout,
        );
    }

    #[test]
    fn classify_unknown_fallback() {
        assert_eq!(
            StepErrorKind::classify("something completely unexpected"),
            StepErrorKind::Unknown,
        );
    }

    #[test]
    fn as_str_round_trips() {
        let kinds = [
            StepErrorKind::MalformedOutput,
            StepErrorKind::ProviderUnreachable,
            StepErrorKind::ProviderDisabled,
            StepErrorKind::LockContention,
            StepErrorKind::Timeout,
            StepErrorKind::Unknown,
        ];
        for kind in kinds {
            // as_str returns the expected label and Display agrees
            assert_eq!(kind.as_str(), format!("{kind}"));
        }
        // spot-check the Unknown → "error" mapping
        assert_eq!(StepErrorKind::Unknown.as_str(), "error");
    }
}
