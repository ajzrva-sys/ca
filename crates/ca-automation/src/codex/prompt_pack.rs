//! Prompt and context pack construction.
//!
//! Target home for role-specific compact context packs. Roles should receive
//! receipts and exact questions, not full repository movies.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ContextPack {
    pub(crate) prompt: String,
    pub(crate) receipt: PromptReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PromptReceipt {
    pub(crate) receipt_id: String,
    pub(crate) role: String,
    #[serde(default)]
    pub(crate) task_id: Option<String>,
    pub(crate) mode: String,
    pub(crate) generated_at: String,
    pub(crate) hard_max_tokens: usize,
    pub(crate) total_bytes: usize,
    pub(crate) total_token_estimate: usize,
    pub(crate) estimated_context_review_seconds: usize,
    pub(crate) rejected: bool,
    pub(crate) rejection_reason: Option<String>,
    pub(crate) sections: Vec<PromptReceiptSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PromptReceiptSection {
    pub(crate) name: String,
    pub(crate) category: PromptSectionCategory,
    pub(crate) priority: PromptSectionPriority,
    pub(crate) reason_included: String,
    pub(crate) max_bytes: usize,
    pub(crate) max_tokens: usize,
    pub(crate) bytes: usize,
    pub(crate) token_estimate: usize,
    pub(crate) truncated: bool,
    pub(crate) deduplicated_from_previous: bool,
    pub(crate) content_hash: String,
    pub(crate) rendered_hash: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PromptSectionCategory {
    RolePrompt,
    TaskText,
    RepoHints,
    FileContents,
    Logs,
    Diff,
    PreviousAttempts,
    Guidelines,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PromptSectionPriority {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone)]
pub(crate) struct ContextPackSection {
    pub(crate) name: String,
    pub(crate) category: PromptSectionCategory,
    pub(crate) priority: PromptSectionPriority,
    pub(crate) reason_included: String,
    pub(crate) content: String,
    pub(crate) max_bytes: usize,
    pub(crate) max_tokens: usize,
    pub(crate) fallback_summary: String,
    pub(crate) deduplicated_from_previous: bool,
    pub(crate) content_hash: Option<String>,
}

impl ContextPackSection {
    pub(crate) fn new(
        name: impl Into<String>,
        category: PromptSectionCategory,
        priority: PromptSectionPriority,
        reason_included: impl Into<String>,
        content: impl Into<String>,
        max_bytes: usize,
        max_tokens: usize,
    ) -> Self {
        Self {
            name: name.into(),
            category,
            priority,
            reason_included: reason_included.into(),
            content: content.into(),
            max_bytes,
            max_tokens,
            fallback_summary: String::new(),
            deduplicated_from_previous: false,
            content_hash: None,
        }
    }

    pub(crate) fn fallback_summary(mut self, summary: impl Into<String>) -> Self {
        self.fallback_summary = summary.into();
        self
    }

    pub(crate) fn deduplicated(mut self, source_hash: String) -> Self {
        self.deduplicated_from_previous = true;
        self.content_hash = Some(source_hash);
        self
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ContextPackBuilder {
    role: String,
    task_id: Option<String>,
    mode: String,
    hard_max_tokens: usize,
    sections: Vec<ContextPackSection>,
}

impl ContextPackBuilder {
    pub(crate) fn new(
        role: impl Into<String>,
        task_id: Option<String>,
        mode: impl Into<String>,
        hard_max_tokens: usize,
    ) -> Self {
        Self {
            role: role.into(),
            task_id,
            mode: mode.into(),
            hard_max_tokens,
            sections: Vec::new(),
        }
    }

    pub(crate) fn section(mut self, section: ContextPackSection) -> Self {
        self.sections.push(section);
        self
    }

    pub(crate) fn build(self) -> ContextPack {
        let mut rendered_sections = Vec::new();
        let mut receipt_sections = Vec::new();
        for section in self.sections {
            let original_hash = section
                .content_hash
                .clone()
                .unwrap_or_else(|| stable_hash_hex(&section.content));
            let (rendered, truncated) = cap_section_content(&section);
            let rendered_hash = stable_hash_hex(&rendered);
            let bytes = rendered.len();
            let token_estimate = estimate_tokens(&rendered);
            receipt_sections.push(PromptReceiptSection {
                name: section.name.clone(),
                category: section.category,
                priority: section.priority,
                reason_included: section.reason_included.clone(),
                max_bytes: section.max_bytes,
                max_tokens: section.max_tokens,
                bytes,
                token_estimate,
                truncated,
                deduplicated_from_previous: section.deduplicated_from_previous,
                content_hash: original_hash,
                rendered_hash,
            });
            rendered_sections.push(format!(
                "## {}\n\nReason: {}\nPriority: {:?}\nCategory: {:?}\n\n{}\n",
                section.name, section.reason_included, section.priority, section.category, rendered
            ));
        }

        let prompt = rendered_sections.join("\n");
        let total_bytes = prompt.len();
        let total_token_estimate = estimate_tokens(&prompt);
        let rejected = total_token_estimate > self.hard_max_tokens;
        let generated_at = Utc::now().to_rfc3339();
        let receipt_id = format!(
            "{}-{}-{}",
            sanitize_receipt_token(&self.role),
            self.task_id
                .as_deref()
                .map(sanitize_receipt_token)
                .unwrap_or_else(|| "repo".to_string()),
            Utc::now().timestamp_millis().unsigned_abs()
        );
        ContextPack {
            prompt,
            receipt: PromptReceipt {
                receipt_id,
                role: self.role,
                task_id: self.task_id,
                mode: self.mode,
                generated_at,
                hard_max_tokens: self.hard_max_tokens,
                total_bytes,
                total_token_estimate,
                estimated_context_review_seconds: estimate_context_review_seconds(
                    total_token_estimate,
                ),
                rejected,
                rejection_reason: rejected.then(|| {
                    format!(
                        "prompt estimate {} exceeded hard max {}",
                        total_token_estimate, self.hard_max_tokens
                    )
                }),
                sections: receipt_sections,
            },
        }
    }
}

fn cap_section_content(section: &ContextPackSection) -> (String, bool) {
    let token_cap_bytes = section.max_tokens.saturating_mul(4);
    let max_bytes = if section.max_bytes == 0 {
        token_cap_bytes
    } else if token_cap_bytes == 0 {
        section.max_bytes
    } else {
        section.max_bytes.min(token_cap_bytes)
    };
    if max_bytes == 0 {
        return (String::new(), !section.content.is_empty());
    }
    if section.content.len() <= max_bytes {
        return (section.content.clone(), false);
    }
    let mut truncated = String::new();
    let mut used = 0usize;
    for ch in section.content.chars() {
        let len = ch.len_utf8();
        if used + len > max_bytes {
            break;
        }
        truncated.push(ch);
        used += len;
    }
    if !section.fallback_summary.trim().is_empty() {
        truncated.push_str("\n\n...[truncated]...\n");
        truncated.push_str(section.fallback_summary.trim());
    } else {
        truncated.push_str("\n\n...[truncated]...");
    }
    (truncated, true)
}

pub(crate) fn estimate_tokens(text: &str) -> usize {
    (text.chars().count() / 4).max(1)
}

pub(crate) fn stable_hash_hex(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn estimate_context_review_seconds(tokens: usize) -> usize {
    // Very rough operator-facing indicator.  The point is not billing accuracy;
    // it is making "this prompt asks the model to read a lot" visible.
    (tokens / 160).max(1)
}

fn sanitize_receipt_token(value: &str) -> String {
    let clean = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    if clean.is_empty() {
        "unknown".to_string()
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_pack_builder_caps_sections_and_records_receipt() {
        let pack = ContextPackBuilder::new("worker", Some("TASK-1".to_string()), "test", 500)
            .section(ContextPackSection::new(
                "Task",
                PromptSectionCategory::TaskText,
                PromptSectionPriority::Critical,
                "selected task",
                "x".repeat(200),
                40,
                20,
            ))
            .build();

        assert!(pack.prompt.contains("...[truncated]..."));
        assert_eq!(pack.receipt.sections.len(), 1);
        assert!(pack.receipt.sections[0].bytes > 40);
        assert!(pack.receipt.sections[0].bytes < 100);
        assert!(pack.receipt.sections[0].truncated);
    }

    #[test]
    fn context_pack_builder_rejects_over_hard_token_budget() {
        let pack = ContextPackBuilder::new("worker", Some("TASK-1".to_string()), "test", 1)
            .section(ContextPackSection::new(
                "Task",
                PromptSectionCategory::TaskText,
                PromptSectionPriority::Critical,
                "selected task",
                "large section",
                1_000,
                1_000,
            ))
            .build();

        assert!(pack.receipt.rejected);
        assert!(pack
            .receipt
            .rejection_reason
            .as_deref()
            .unwrap_or_default()
            .contains("hard max"));
    }
}
