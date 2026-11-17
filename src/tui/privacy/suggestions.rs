//! Local suggestion analysis, including the complete supplied conversation.

use regex::Regex;
use std::sync::{Arc, OnceLock};

const MAX_SCAN_BYTES: usize = 64 * 1024 * 1024;
const MAX_TERMS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    Phrase,
    Identifier,
}

#[derive(Debug, Clone)]
pub struct SuggestedTerm {
    pub text: String,
    pub kind: MatchKind,
    pub spellings: Vec<String>,
    pub occurrences: usize,
    pub historical: usize,
}

#[derive(Debug, Clone)]
pub struct GuideAnalysis {
    pub terms: Vec<SuggestedTerm>,
    pub has_urls: bool,
    pub has_attachment: bool,
    pub has_task_list: bool,
    pub scanned_bytes: usize,
    pub scanned_parts: usize,
    pub complete: bool,
}

#[derive(Clone)]
pub struct GuideInput {
    pub task: String,
    pub expanded: String,
    pub history: Vec<String>,
    pub imported: Option<Arc<crate::provider_history::NativeHistory>>,
}

impl std::fmt::Debug for GuideInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuideInput")
            .field("history_parts", &self.history.len())
            .field("has_imported_history", &self.imported.is_some())
            .finish_non_exhaustive()
    }
}

impl GuideInput {
    pub fn analyze(&self) -> GuideAnalysis {
        let imported = self
            .imported
            .iter()
            .flat_map(|history| history.text_chunks());
        analyze(
            &self.task,
            &self.expanded,
            self.history.iter().map(String::as_str).chain(imported),
        )
    }
}

impl GuideAnalysis {
    pub fn needs_review(&self) -> bool {
        self.has_attachment
            || self.has_urls
            || self.has_task_list
            || !self.terms.is_empty()
            || !self.complete
    }

    pub fn omit_active(&mut self, filter: &crate::provider_filter::ProviderFilter) {
        self.has_urls &= !filter.hides_url_hosts();
        self.terms.retain_mut(|term| match term.kind {
            MatchKind::Phrase => !filter.covers_hidden_word(&term.text),
            MatchKind::Identifier => {
                term.spellings
                    .retain(|spelling| !filter.covers_session_identifier(spelling));
                !term.spellings.is_empty()
            }
        });
    }
}

fn matcher(identifier: bool) -> &'static Regex {
    static PHRASES: OnceLock<Regex> = OnceLock::new();
    static IDENTIFIERS: OnceLock<Regex> = OnceLock::new();
    if identifier {
        IDENTIFIERS.get_or_init(|| {
            Regex::new(concat!(
                r"(?i)VMXNET3|VMkernel|ESXi|bhyve|VMCS|NVMe|copyin|tcpdump|",
                r"Ivy Bridge|Haswell|EPT|NMI|VMX"
            ))
            .expect("static project-label expression")
        })
    } else {
        PHRASES.get_or_init(|| {
            Regex::new(concat!(
                r"(?i)\b(?:ransomware|malware|exploits?|exploitation|payloads?|phishing|",
                r"vulnerabilit(?:y|ies)|penetration testing|privilege escalation|",
                r"sql injection|cross[- ]site scripting|remote code execution|",
                r"denial of service|command and control|reverse shells?|shellcode|",
                r"credential theft|confidential|private key|api key|passwords?)\b"
            ))
            .expect("static task phrase expression")
        })
    }
}

fn boundary(text: &str, start: usize, end: usize) -> bool {
    !text[..start]
        .chars()
        .next_back()
        .is_some_and(char::is_alphanumeric)
        && !text[end..]
            .chars()
            .next()
            .is_some_and(char::is_alphanumeric)
}

pub fn analyze<'a>(
    task: &str,
    expanded: &str,
    history: impl Iterator<Item = &'a str>,
) -> GuideAnalysis {
    let mut result = GuideAnalysis {
        terms: Vec::new(),
        has_urls: false,
        has_task_list: pulldown_cmark::Parser::new(task)
            .filter(|event| {
                matches!(
                    event,
                    pulldown_cmark::Event::Start(pulldown_cmark::Tag::Item)
                )
            })
            .take(2)
            .count()
            == 2,
        has_attachment: task != expanded
            || task
                .split_whitespace()
                .any(|part| part.starts_with('@') && part.len() > 1),
        scanned_bytes: 0,
        scanned_parts: 0,
        complete: true,
    };
    for (text, historical) in std::iter::once((expanded, false)).chain(history.map(|t| (t, true))) {
        if result.scanned_bytes.saturating_add(text.len()) > MAX_SCAN_BYTES {
            result.complete = false;
            break;
        }
        result.scanned_bytes += text.len();
        result.scanned_parts += 1;
        let lower = text.to_ascii_lowercase();
        result.has_urls |= lower.contains("https://") || lower.contains("http://");
        for kind in [MatchKind::Phrase, MatchKind::Identifier] {
            for found in matcher(kind == MatchKind::Identifier).find_iter(text) {
                if kind == MatchKind::Identifier && !boundary(text, found.start(), found.end()) {
                    continue;
                }
                let existing = result.terms.iter_mut().find(|term| {
                    term.kind == kind && term.text.eq_ignore_ascii_case(found.as_str())
                });
                if let Some(term) = existing {
                    term.occurrences += 1;
                    term.historical += usize::from(historical);
                    if !term
                        .spellings
                        .iter()
                        .any(|spelling| spelling == found.as_str())
                    {
                        term.spellings.push(found.as_str().to_owned());
                    }
                } else if result.terms.len() < MAX_TERMS {
                    result.terms.push(SuggestedTerm {
                        text: found.as_str().to_owned(),
                        kind,
                        spellings: vec![found.as_str().to_owned()],
                        occurrences: 1,
                        historical: usize::from(historical),
                    });
                } else {
                    result.complete = false;
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_history_suggestions_cover_tail_tool_data_and_exact_variants() {
        let old = "ordinary previous text ".repeat(20_000);
        let history = [
            old.as_str(),
            "{\"command\":\"EPT_WALK ept NMI VMkernel VMXNET3\"}",
            "kept excerpts αEPT EPTβ",
        ];
        let result = analyze("Continue", "Continue", history.into_iter());
        assert!(result.complete && result.scanned_bytes > 256 * 1024);
        assert_eq!(result.scanned_parts, 4);
        let ept = result.terms.iter().find(|term| term.text == "EPT").unwrap();
        assert_eq!(ept.occurrences, 2);
        assert_eq!(ept.historical, 2);
        assert_eq!(ept.spellings, ["EPT", "ept"]);
        assert!(result.terms.iter().any(|term| term.text == "VMXNET3"));
    }

    #[test]
    fn full_history_suggestions_do_not_duplicate_task_or_invent_labels() {
        let result = analyze(
            "Review @notes.txt",
            "Review @notes.txt\nVMkernel malware",
            std::iter::empty(),
        );
        assert_eq!(result.terms.len(), 2);
        assert!(result.terms.iter().all(|term| term.occurrences == 1));
        assert!(result.has_attachment && result.needs_review());
        assert!(!analyze("Fix the menu", "Fix the menu", std::iter::empty()).needs_review());
    }
}
