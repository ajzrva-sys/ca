//! Conditional, inert user and workspace instruction rules.
//!
//! Rules only contribute text to the host-built system prompt. Activation is
//! derived from host-observed explicit file references or saved user state;
//! prompt prose and model output are never inspected for rule selection.

use globset::{GlobBuilder, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_RULE_BYTES: u64 = 64 * 1024;
const MAX_RULES: usize = 256;
const MAX_GLOBS: usize = 16;
const MAX_GLOB_BYTES: usize = 256;
const MAX_DESCRIPTION_BYTES: usize = 240;
const MAX_DISCOVERY_DEPTH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RuleActivation {
    Always,
    Glob,
    Manual,
}

impl RuleActivation {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Glob => "glob",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuleScope {
    User,
    Workspace {
        root_id: crate::workspace::RootId,
        root_index: usize,
    },
}

impl RuleScope {
    pub(crate) fn label(&self) -> String {
        match self {
            Self::User => "user".into(),
            Self::Workspace { root_index, .. } => format!("workspace-{root_index}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rule {
    pub id: String,
    pub name: String,
    pub description: String,
    pub activation: RuleActivation,
    pub globs: Vec<String>,
    pub body: String,
    pub path: PathBuf,
    pub root: Option<PathBuf>,
    pub scope: RuleScope,
    pub digest: String,
    pub legacy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuleDiscoveryError {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RuleRegistry {
    rules: Vec<Rule>,
    errors: Vec<RuleDiscoveryError>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RuleFrontmatter {
    description: String,
    activation: RuleActivation,
    #[serde(default)]
    globs: Vec<String>,
}

pub(crate) fn parse_activation_spec(value: &str) -> Result<(RuleActivation, Vec<String>), String> {
    let (activation, globs) = if value == "always" {
        (RuleActivation::Always, Vec::new())
    } else if value == "manual" {
        (RuleActivation::Manual, Vec::new())
    } else if let Some(patterns) = value.strip_prefix("glob:") {
        (
            RuleActivation::Glob,
            patterns
                .split(',')
                .map(str::trim)
                .filter(|pattern| !pattern.is_empty())
                .map(str::to_string)
                .collect(),
        )
    } else {
        return Err("activation must be always, manual, or glob:<pattern>[,<pattern>]".into());
    };
    validate_globs(activation, &globs)?;
    Ok((activation, globs))
}

pub(crate) fn render_rule_document(
    description: &str,
    activation: RuleActivation,
    globs: Vec<String>,
    body: &str,
) -> Result<String, String> {
    validate_description(description)?;
    validate_globs(activation, &globs)?;
    validate_rule_text(body)?;
    if body.trim().is_empty() {
        return Err("rule body is empty".into());
    }
    let yaml = serde_yaml_ng::to_string(&RuleFrontmatter {
        description: description.into(),
        activation,
        globs,
    })
    .map_err(|_| "rule frontmatter could not be rendered")?;
    Ok(format!("---\n{}---\n\n{}\n", yaml, body.trim()))
}

pub(crate) fn validate_name(name: &str) -> Result<(), String> {
    validate_rule_name(name)
}

impl RuleRegistry {
    pub(crate) fn discover(
        paths: &crate::runtime::RuntimePaths,
        workspace: &crate::workspace::WorkspaceContext,
    ) -> Self {
        Self::discover_from_home(&paths.home, workspace)
    }

    pub(crate) fn discover_from_home(
        home: &Path,
        workspace: &crate::workspace::WorkspaceContext,
    ) -> Self {
        let mut sources = vec![(RuleScope::User, home.join(".ca/rules"), None)];
        sources.extend(
            workspace
                .roots()
                .iter()
                .enumerate()
                .map(|(root_index, root)| {
                    (
                        RuleScope::Workspace {
                            root_id: root.id,
                            root_index,
                        },
                        root.path.join(".ca/rules"),
                        Some(root.path.clone()),
                    )
                }),
        );
        Self::discover_sources(&sources)
    }

    pub(crate) fn discover_workspace_only(workspace: &crate::workspace::WorkspaceContext) -> Self {
        let sources = workspace
            .roots()
            .iter()
            .enumerate()
            .map(|(root_index, root)| {
                (
                    RuleScope::Workspace {
                        root_id: root.id,
                        root_index,
                    },
                    root.path.join(".ca/rules"),
                    Some(root.path.clone()),
                )
            })
            .collect::<Vec<_>>();
        Self::discover_sources(&sources)
    }

    pub(crate) fn discover_user_only(home: &Path) -> Self {
        Self::discover_sources(&[(RuleScope::User, home.join(".ca/rules"), None)])
    }

    fn discover_sources(sources: &[(RuleScope, PathBuf, Option<PathBuf>)]) -> Self {
        let mut registry = Self::default();
        let mut seen_ids = BTreeSet::new();
        for (scope, directory, root) in sources {
            let mut files = Vec::new();
            collect_markdown(directory, directory, &mut files, 0);
            files.sort();
            for path in files {
                if registry.rules.len() >= MAX_RULES {
                    registry.errors.push(RuleDiscoveryError {
                        path: directory.clone(),
                        reason: "rule catalog exceeds 256 entries".into(),
                    });
                    return registry;
                }
                match parse_rule(&path, scope.clone(), root.clone()) {
                    Ok(rule) if seen_ids.insert(rule.id.clone()) => registry.rules.push(rule),
                    Ok(rule) => registry.errors.push(RuleDiscoveryError {
                        path,
                        reason: format!("duplicate rule id {}", rule.id),
                    }),
                    Err(reason) => registry.errors.push(RuleDiscoveryError { path, reason }),
                }
            }
        }
        registry
    }

    pub(crate) fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub(crate) fn errors(&self) -> &[RuleDiscoveryError] {
        &self.errors
    }

    pub(crate) fn find(&self, id: &str) -> Option<&Rule> {
        self.rules.iter().find(|rule| rule.id == id)
    }

    pub(crate) fn retain_valid_manual_ids(&self, ids: &mut BTreeSet<String>) {
        ids.retain(|id| {
            self.find(id)
                .is_some_and(|rule| rule.activation == RuleActivation::Manual)
        });
    }

    pub(crate) fn is_active(
        &self,
        rule: &Rule,
        manual_ids: &BTreeSet<String>,
        explicit_paths: &[PathBuf],
        workspace: &crate::workspace::WorkspaceContext,
    ) -> bool {
        match rule.activation {
            RuleActivation::Always => true,
            RuleActivation::Manual => manual_ids.contains(&rule.id),
            RuleActivation::Glob => glob_rule_matches(rule, explicit_paths, workspace),
        }
    }

    pub(crate) fn render_active(
        &self,
        manual_ids: &BTreeSet<String>,
        explicit_paths: &[PathBuf],
        workspace: &crate::workspace::WorkspaceContext,
    ) -> String {
        self.rules
            .iter()
            .filter(|rule| self.is_active(rule, manual_ids, explicit_paths, workspace))
            .map(|rule| {
                format!(
                    "# Rule: {} [{}] — {}\n{}",
                    rule.name,
                    rule.id,
                    rule.description,
                    rule.body.trim()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    #[cfg(test)]
    fn discover_for_test(user: PathBuf, workspace: &crate::workspace::WorkspaceContext) -> Self {
        let mut sources = vec![(RuleScope::User, user, None)];
        sources.extend(
            workspace
                .roots()
                .iter()
                .enumerate()
                .map(|(root_index, root)| {
                    (
                        RuleScope::Workspace {
                            root_id: root.id,
                            root_index,
                        },
                        root.path.join(".ca/rules"),
                        Some(root.path.clone()),
                    )
                }),
        );
        Self::discover_sources(&sources)
    }
}

fn collect_markdown(root: &Path, current: &Path, output: &mut Vec<PathBuf>, depth: usize) {
    if depth > MAX_DISCOVERY_DEPTH || output.len() >= MAX_RULES {
        return;
    }
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            if !entry.file_name().to_string_lossy().starts_with('.') {
                collect_markdown(root, &path, output, depth + 1);
            }
        } else if metadata.is_file()
            && metadata.len() <= MAX_RULE_BYTES
            && path.extension().is_some_and(|extension| extension == "md")
            && path.starts_with(root)
        {
            output.push(path);
        }
    }
}

fn parse_rule(path: &Path, scope: RuleScope, root: Option<PathBuf>) -> Result<Rule, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "rule could not be inspected")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_RULE_BYTES {
        return Err("rule is not a bounded regular file".into());
    }
    let name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "rule name is invalid".to_string())?
        .to_ascii_lowercase();
    validate_rule_name(&name)?;
    let bytes = fs::read(path).map_err(|_| "rule could not be read")?;
    if bytes.len() > MAX_RULE_BYTES as usize {
        return Err("rule changed while reading".into());
    }
    let content = std::str::from_utf8(&bytes).map_err(|_| "rule is not UTF-8")?;
    validate_rule_text(content)?;
    let (description, activation, globs, body, legacy) = if content.trim_start().starts_with("---")
    {
        let (frontmatter, body) = split_frontmatter(content)?;
        let frontmatter: RuleFrontmatter = serde_yaml_ng::from_str(frontmatter)
            .map_err(|_| "rule frontmatter is invalid".to_string())?;
        validate_description(&frontmatter.description)?;
        validate_globs(frontmatter.activation, &frontmatter.globs)?;
        let body = body.trim().to_string();
        if body.is_empty() {
            return Err("rule body is empty".into());
        }
        (
            frontmatter.description,
            frontmatter.activation,
            frontmatter.globs,
            body,
            false,
        )
    } else {
        let body = content.trim().to_string();
        if body.is_empty() {
            return Err("rule body is empty".into());
        }
        (name.clone(), RuleActivation::Always, Vec::new(), body, true)
    };
    let id = format!("{}/{}", scope.label(), name);
    Ok(Rule {
        id,
        name,
        description,
        activation,
        globs,
        body,
        path: path.to_path_buf(),
        root,
        scope,
        digest: digest(&bytes),
        legacy,
    })
}

fn split_frontmatter(content: &str) -> Result<(&str, &str), String> {
    let rest = content
        .strip_prefix("---\r\n")
        .or_else(|| content.strip_prefix("---\n"))
        .ok_or_else(|| "rule frontmatter must start on the first line".to_string())?;
    for marker in ["\n---\n", "\r\n---\r\n"] {
        if let Some(end) = rest.find(marker) {
            return Ok((&rest[..end], &rest[end + marker.len()..]));
        }
    }
    Err("rule frontmatter is unterminated".into())
}

fn validate_rule_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 64
        || !name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        return Err("rule name must use lowercase letters, digits, hyphens, or underscores".into());
    }
    Ok(())
}

fn validate_description(description: &str) -> Result<(), String> {
    if description.is_empty()
        || description.len() > MAX_DESCRIPTION_BYTES
        || description.chars().any(|character| character.is_control())
    {
        return Err("rule description is unsafe or too large".into());
    }
    Ok(())
}

fn validate_rule_text(text: &str) -> Result<(), String> {
    if text.len() > MAX_RULE_BYTES as usize
        || text
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err("rule text is unsafe or too large".into());
    }
    Ok(())
}

fn validate_globs(activation: RuleActivation, globs: &[String]) -> Result<(), String> {
    match activation {
        RuleActivation::Glob if globs.is_empty() => {
            return Err("glob rules require at least one glob".into());
        }
        RuleActivation::Always | RuleActivation::Manual if !globs.is_empty() => {
            return Err("only glob rules may declare globs".into());
        }
        _ => {}
    }
    if globs.len() > MAX_GLOBS {
        return Err("rule declares too many globs".into());
    }
    for pattern in globs {
        if pattern.is_empty()
            || pattern.len() > MAX_GLOB_BYTES
            || Path::new(pattern).is_absolute()
            || pattern.split('/').any(|component| component == "..")
            || pattern.chars().any(|character| character.is_control())
        {
            return Err("rule glob is unsafe".into());
        }
        GlobBuilder::new(pattern)
            .literal_separator(true)
            .backslash_escape(false)
            .build()
            .map_err(|_| "rule glob is invalid".to_string())?;
    }
    Ok(())
}

fn glob_rule_matches(
    rule: &Rule,
    explicit_paths: &[PathBuf],
    workspace: &crate::workspace::WorkspaceContext,
) -> bool {
    let mut builder = GlobSetBuilder::new();
    for pattern in &rule.globs {
        let Ok(glob) = GlobBuilder::new(pattern)
            .literal_separator(true)
            .backslash_escape(false)
            .build()
        else {
            return false;
        };
        builder.add(glob);
    }
    let Ok(patterns) = builder.build() else {
        return false;
    };
    explicit_paths.iter().any(|path| {
        let candidate_roots = match &rule.scope {
            RuleScope::Workspace { root_id, .. } => workspace
                .roots()
                .iter()
                .filter(|root| &root.id == root_id)
                .collect::<Vec<_>>(),
            RuleScope::User => workspace.roots().iter().collect::<Vec<_>>(),
        };
        candidate_roots.into_iter().any(|root| {
            path.strip_prefix(&root.path).ok().is_some_and(|relative| {
                let relative = relative.to_string_lossy().replace('\\', "/");
                !relative.is_empty() && patterns.is_match(relative)
            })
        })
    })
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_rule(root: &Path, name: &str, content: &str) {
        fs::create_dir_all(root).unwrap();
        fs::write(root.join(format!("{name}.md")), content).unwrap();
    }

    #[test]
    fn legacy_always_glob_and_manual_rules_use_only_explicit_paths() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let user = temp.path().join("user-rules");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
        write_rule(&project.join(".ca/rules"), "legacy", "legacy body");
        write_rule(
            &project.join(".ca/rules"),
            "rust",
            "---\ndescription: Rust files\nactivation: glob\nglobs: [\"**/*.rs\"]\n---\nrust body",
        );
        write_rule(
            &user,
            "manual",
            "---\ndescription: Manual choice\nactivation: manual\n---\nmanual body",
        );
        let workspace = crate::workspace::WorkspaceContext::discover(&project).unwrap();
        let registry = RuleRegistry::discover_for_test(user, &workspace);
        assert!(registry.errors().is_empty());

        let mut manual = BTreeSet::new();
        let no_explicit = registry.render_active(&manual, &[], &workspace);
        assert!(no_explicit.contains("legacy body"));
        assert!(!no_explicit.contains("rust body"));
        assert!(!no_explicit.contains("manual body"));

        let prose_path_is_not_an_explicit_path = registry.render_active(&manual, &[], &workspace);
        assert!(!prose_path_is_not_an_explicit_path.contains("rust body"));
        let explicit = fs::canonicalize(project.join("src/main.rs")).unwrap();
        let matched = registry.render_active(&manual, &[explicit], &workspace);
        assert!(matched.contains("rust body"));

        manual.insert("user/manual".into());
        let activated = registry.render_active(&manual, &[], &workspace);
        assert!(activated.contains("manual body"));
    }

    #[test]
    fn glob_rules_are_scoped_to_their_owning_workspace_root() {
        let temp = tempfile::tempdir().unwrap();
        let primary = temp.path().join("primary");
        let additional = temp.path().join("additional");
        fs::create_dir_all(primary.join("src")).unwrap();
        fs::create_dir_all(additional.join("src")).unwrap();
        fs::write(primary.join("src/one.rs"), "one").unwrap();
        fs::write(additional.join("src/two.rs"), "two").unwrap();
        write_rule(
            &primary.join(".ca/rules"),
            "primary-rust",
            "---\ndescription: Primary Rust\nactivation: glob\nglobs: [\"src/*.rs\"]\n---\nprimary-only",
        );
        let workspace = crate::workspace::WorkspaceContext::from_roots(
            &primary,
            &[fs::canonicalize(&additional).unwrap()],
        )
        .unwrap();
        let registry = RuleRegistry::discover_for_test(temp.path().join("none"), &workspace);
        let manual = BTreeSet::new();
        let other = fs::canonicalize(additional.join("src/two.rs")).unwrap();
        assert!(!registry
            .render_active(&manual, &[other], &workspace)
            .contains("primary-only"));
        let own = fs::canonicalize(primary.join("src/one.rs")).unwrap();
        assert!(registry
            .render_active(&manual, &[own], &workspace)
            .contains("primary-only"));
    }

    #[test]
    fn malformed_metadata_globs_symlinks_and_bounds_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        for (name, content) in [
            (
                "unknown",
                "---\ndescription: Bad\nactivation: always\nextra: nope\n---\nbody",
            ),
            (
                "missing-glob",
                "---\ndescription: Bad\nactivation: glob\n---\nbody",
            ),
            (
                "glob-on-always",
                "---\ndescription: Bad\nactivation: always\nglobs: [\"*.rs\"]\n---\nbody",
            ),
            (
                "escape",
                "---\ndescription: Bad\nactivation: glob\nglobs: [\"../*.rs\"]\n---\nbody",
            ),
        ] {
            write_rule(&project.join(".ca/rules"), name, content);
        }
        let outside = temp.path().join("outside.md");
        fs::write(&outside, "outside").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, project.join(".ca/rules/link.md")).unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(&project).unwrap();
        let registry = RuleRegistry::discover_for_test(temp.path().join("none"), &workspace);
        assert!(registry.rules().is_empty());
        assert_eq!(registry.errors().len(), 4);
    }

    #[test]
    fn user_and_every_workspace_root_are_discovered_in_stable_order() {
        let temp = tempfile::tempdir().unwrap();
        let primary = temp.path().join("primary");
        let additional = temp.path().join("additional");
        let user = temp.path().join("user");
        fs::create_dir_all(&primary).unwrap();
        fs::create_dir_all(&additional).unwrap();
        write_rule(&user, "user", "user body");
        write_rule(&primary.join(".ca/rules"), "primary", "primary body");
        write_rule(
            &additional.join(".ca/rules"),
            "additional",
            "additional body",
        );
        let workspace = crate::workspace::WorkspaceContext::from_roots(
            &primary,
            &[fs::canonicalize(&additional).unwrap()],
        )
        .unwrap();
        let registry = RuleRegistry::discover_for_test(user, &workspace);
        assert_eq!(
            registry
                .rules()
                .iter()
                .map(|rule| rule.id.as_str())
                .collect::<Vec<_>>(),
            ["user/user", "workspace-0/primary", "workspace-1/additional"]
        );
    }

    #[test]
    fn user_only_discovery_excludes_project_rules() {
        let temp = tempfile::tempdir().unwrap();
        write_rule(&temp.path().join(".ca/rules"), "user", "user body");
        write_rule(
            &temp.path().join("project/.ca/rules"),
            "project",
            "project body",
        );
        let registry = RuleRegistry::discover_user_only(temp.path());
        assert_eq!(registry.rules().len(), 1);
        assert_eq!(registry.rules()[0].id, "user/user");
    }
}
