//! User-owned mechanical disclosure rules. No rule is inferred from prose.
use globset::{GlobBuilder, GlobMatcher};
use serde::{Deserialize, Serialize};

pub const POLICY_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub version: u32,
    pub enabled: bool,
    pub revision: u64,
    pub rules: Vec<Rule>,
    #[serde(default, skip_serializing_if = "Action::is_hide")]
    pub default_action: Action,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            version: POLICY_VERSION,
            enabled: false,
            revision: 0,
            rules: Vec::new(),
            default_action: Action::Hide,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub file: String,
    pub selector: String,
    pub action: Action,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Allow,
    Alias,
    #[default]
    Hide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AliasKind {
    Text,
    Key,
    Identifier,
    String,
    Number,
    Boolean,
    Null,
    Comment,
    Path,
    Opaque,
}

impl Action {
    fn is_hide(&self) -> bool {
        *self == Self::Hide
    }
}

impl AliasKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Key => "key",
            Self::Identifier => "identifier",
            Self::String => "string",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Null => "null",
            Self::Comment => "comment",
            Self::Path => "path",
            Self::Opaque => "opaque",
        }
    }
}

fn pattern(value: &str) -> Result<GlobMatcher, String> {
    GlobBuilder::new(value)
        .literal_separator(true)
        .backslash_escape(false)
        .build()
        .map(|glob| glob.compile_matcher())
        .map_err(|_| "invalid privacy rule pattern".to_string())
}

impl Policy {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != POLICY_VERSION {
            return Err("unsupported privacy policy version".into());
        }
        if self.rules.len() > 4096 {
            return Err("privacy policy exceeds 4096 rules".into());
        }
        for rule in &self.rules {
            if rule.file.is_empty()
                || rule.selector.is_empty()
                || rule.file.len() > 4096
                || rule.selector.len() > 4096
            {
                return Err(
                    "privacy rule requires bounded file and structural selector patterns".into(),
                );
            }
            pattern(&rule.file)?;
            pattern(&rule.selector)?;
        }
        Ok(())
    }

    /// Later matching rules override earlier rules. Unmatched data stays private.
    pub fn action_for(&self, path: &str, selector: &str) -> Action {
        self.for_path(path).action_for(selector)
    }

    pub fn for_path(&self, path: &str) -> PathPolicy {
        let path = path.replace('\\', "/");
        let rules = self
            .rules
            .iter()
            .filter_map(|rule| {
                let file = pattern(&rule.file).ok()?;
                let basename = path.rsplit('/').next().unwrap_or(&path);
                if !file.is_match(&path) && !(rule.file == "*" && file.is_match(basename)) {
                    return None;
                }
                Some((
                    pattern(&rule.selector).ok()?,
                    rule.selector.clone(),
                    rule.action,
                ))
            })
            .collect();
        PathPolicy {
            rules,
            default_action: self.default_action,
        }
    }
}

pub struct PathPolicy {
    rules: Vec<(GlobMatcher, String, Action)>,
    default_action: Action,
}

impl PathPolicy {
    pub fn exact_boundaries(&self, digest: &str, source: &str) -> Vec<usize> {
        let mut boundaries = Vec::new();
        for (_, selector, _) in &self.rules {
            if let Some(range) = source_range(selector, digest, source) {
                boundaries.push(range.start);
                boundaries.push(range.end);
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        boundaries
    }

    #[cfg(test)]
    pub fn action_for_source(
        &self,
        selector: &str,
        exact: &str,
        digest: &str,
        source: &str,
    ) -> Action {
        self.selected_action_for_source(selector, exact, digest, source)
            .unwrap_or(self.default_action)
    }

    pub fn selected_action_for_source(
        &self,
        selector: &str,
        exact: &str,
        digest: &str,
        source: &str,
    ) -> Option<Action> {
        let selected = source_range(exact, digest, source);
        self.rules
            .iter()
            .rev()
            .find_map(|(matcher, prefix, action)| {
                if prefix.starts_with("/@digest/") {
                    let range = source_range(prefix, digest, source)?;
                    return selected
                        .as_ref()
                        .filter(|selected| {
                            range.start <= selected.start && selected.end <= range.end
                        })
                        .map(|_| *action);
                }
                (matcher.is_match(selector)
                    || matcher.is_match(exact)
                    || (selector.starts_with(prefix)
                        && selector.as_bytes().get(prefix.len()) == Some(&b'/')))
                .then_some(*action)
            })
    }
    pub fn action_for(&self, selector: &str) -> Action {
        self.rules
            .iter()
            .rev()
            .find_map(|(matcher, prefix, action)| {
                (matcher.is_match(selector)
                    || (selector.starts_with(prefix)
                        && selector.as_bytes().get(prefix.len()) == Some(&b'/')))
                .then_some(*action)
            })
            .unwrap_or(self.default_action)
    }

    #[cfg(test)]
    pub fn action_for_exact(&self, selector: &str, exact: &str) -> Action {
        self.rules
            .iter()
            .rev()
            .find_map(|(matcher, prefix, action)| {
                (matcher.is_match(selector)
                    || matcher.is_match(exact)
                    || (selector.starts_with(prefix)
                        && selector.as_bytes().get(prefix.len()) == Some(&b'/')))
                .then_some(*action)
            })
            .unwrap_or(self.default_action)
    }
}

fn source_range(selector: &str, digest: &str, source: &str) -> Option<std::ops::Range<usize>> {
    let rest = selector.strip_prefix("/@digest/")?;
    let (identity, rest) = rest.split_once('/')?;
    if identity != digest {
        return None;
    }
    let (start, end) = rest.split_once('/')?;
    let start = start.parse::<usize>().ok()?;
    let end = end.parse::<usize>().ok()?;
    (start < end
        && end <= source.len()
        && source.is_char_boundary(start)
        && source.is_char_boundary(end))
    .then_some(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_private_and_last_rule_wins_with_structural_reuse() {
        let policy = Policy {
            enabled: true,
            rules: vec![
                Rule {
                    file: "docs/*.json".into(),
                    selector: "/object/pair/0/**".into(),
                    action: Action::Allow,
                },
                Rule {
                    file: "docs/*.json".into(),
                    selector: "/object/pair/0/value".into(),
                    action: Action::Hide,
                },
            ],
            ..Policy::default()
        };
        policy.validate().unwrap();
        assert_eq!(
            policy.action_for("docs/a.json", "/object/pair/0/key"),
            Action::Allow
        );
        assert_eq!(
            policy.action_for("docs/b.json", "/object/pair/0/value"),
            Action::Hide
        );
        assert_eq!(
            policy.action_for("other/a.json", "/object/pair/0/key"),
            Action::Hide
        );
    }
    #[test]
    fn malformed_or_future_rules_are_rejected() {
        let mut policy = Policy::default();
        policy.version += 1;
        assert!(policy.validate().is_err());
        policy.version = POLICY_VERSION;
        policy.rules.push(Rule {
            file: "[".into(),
            selector: "/**".into(),
            action: Action::Allow,
        });
        assert!(policy.validate().is_err());
    }
}
