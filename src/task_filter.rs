//! Reviewed, task-owned filtering. Semantic additions are fixed at Send.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Preset {
    #[default]
    PrivateValues,
    PrivateValuesAndSuggestions,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Policy {
    #[serde(default)]
    pub preset: Preset,
    #[serde(default)]
    pub words: Vec<String>,
    #[serde(default)]
    pub identifiers: Vec<String>,
}

impl Policy {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.words.len() > 32 || self.identifiers.len() > 128 {
            return Err("Use at most 32 terms and 128 exact identifiers per task.".into());
        }
        for text in self.words.iter().chain(&self.identifiers) {
            if text.is_empty()
                || text.len() > 128
                || text.trim() != text
                || text.chars().any(char::is_control)
                || !text.chars().any(char::is_alphanumeric)
            {
                return Err("Each filter term must contain 1–128 bytes of printable text.".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Preview {
    pub original: String,
    pub provider_copy: String,
    pub changes: String,
    pub coverage: String,
    pub blocked: Option<String>,
    /// Local basename → provider alias rows. Never copied into `provider_copy`.
    pub filename_samples: Vec<(String, String)>,
}

pub(crate) fn describe_changes(original: &str, projected: &str) -> String {
    if original == projected {
        return "No matching values in the known input. The preset also applies to later reads."
            .into();
    }
    let mut before = original.lines();
    let mut after = projected.lines();
    let mut output = String::new();
    let mut changes = 0;
    loop {
        let pair = (before.next(), after.next());
        if pair == (None, None) {
            break;
        }
        if pair.0 == pair.1 {
            continue;
        }
        if changes == 200 || output.len() >= 60 * 1024 {
            output.push_str("\nMore changes omitted here; compare Original and Provider copy for the complete input.\n");
            break;
        }
        changes += 1;
        output.push_str(if pair.1.is_none_or(str::is_empty) {
            "Omitted\n"
        } else {
            "Replaced\n"
        });
        for (prefix, line) in [("− ", pair.0), ("+ ", pair.1)] {
            if let Some(line) = line {
                output.push_str(prefix);
                output.extend(line.chars().take(4096));
                if line.chars().count() > 4096 {
                    output.push_str("… [line shortened]");
                }
                output.push('\n');
            }
        }
        output.push('\n');
    }
    output
}

pub(crate) const COVERAGE: &str = "Covers discovered workspace filenames, URL hosts, supported web page data, and saved source selections. URL paths and queries remain readable unless another rule selects them. Other source text stays readable unless explicitly selected. Reviewed terms apply to later reads without adding new suggestions.";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectEdits {
    #[serde(default)]
    pub additions: Policy,
    #[serde(default)]
    pub exclusions: Policy,
}

impl ProjectEdits {
    pub(crate) fn validate(&self) -> Result<(), String> {
        self.additions.validate()?;
        self.exclusions.validate()
    }

    pub(crate) fn apply(&self, suggested: &mut Policy) {
        fn apply_terms(terms: &mut Vec<String>, added: &[String], removed: &[String]) {
            terms.retain(|term| !removed.contains(term));
            for term in added {
                if !terms.contains(term) {
                    terms.push(term.clone());
                }
            }
        }
        apply_terms(
            &mut suggested.words,
            &self.additions.words,
            &self.exclusions.words,
        );
        apply_terms(
            &mut suggested.identifiers,
            &self.additions.identifiers,
            &self.exclusions.identifiers,
        );
    }

    pub(crate) fn remember(&mut self, suggested: &Policy, selected: &Policy) {
        fn merge(
            added: &mut Vec<String>,
            removed: &mut Vec<String>,
            before: &[String],
            after: &[String],
        ) {
            for term in before {
                if !after.contains(term) {
                    added.retain(|existing| existing != term);
                    if !removed.contains(term) {
                        removed.push(term.clone());
                    }
                }
            }
            for term in after {
                removed.retain(|existing| existing != term);
                if !added.contains(term) {
                    added.push(term.clone());
                }
            }
        }
        merge(
            &mut self.additions.words,
            &mut self.exclusions.words,
            &suggested.words,
            &selected.words,
        );
        merge(
            &mut self.additions.identifiers,
            &mut self.exclusions.identifiers,
            &suggested.identifiers,
            &selected.identifiers,
        );
    }
}

/// Kept only in the existing private alias registry, never in provider input.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub version: u32,
    pub task_id: uuid::Uuid,
    pub policy: Policy,
    pub restorations: Vec<(String, String)>,
    pub urls: Vec<String>,
}

impl Record {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.task_id.is_nil() {
            return Err("Saved task filter is invalid.".into());
        }
        self.policy.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_filter_policy_defaults_and_exact_selected_terms() {
        let policy: Policy = serde_json::from_str("{}").unwrap();
        assert_eq!(policy.preset, Preset::PrivateValues);
        assert!(policy.words.is_empty());
        assert!(policy.identifiers.is_empty());
        assert!(Policy {
            words: vec![" bad ".into()],
            ..Default::default()
        }
        .validate()
        .is_err());
    }
}
