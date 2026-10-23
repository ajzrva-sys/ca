use glob::Pattern;
use serde::{Deserialize, Serialize};

const MAX_TOOL_NAME_BYTES: usize = 128;
const MAX_PATH_PATTERN_BYTES: usize = 1024;
const MAX_COMMAND_TOKENS: usize = 64;
const MAX_COMMAND_BYTES: usize = 4096;
const MAX_PREFIX_TOKENS: usize = 8;
const MAX_PREFIX_TOKEN_BYTES: usize = 256;
const MAX_SESSION_GRANTS: usize = 64;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Ask,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PermissionMatcher {
    Tool {
        tool: String,
    },
    Path {
        tool: String,
        path_pattern: String,
    },
    CommandPrefix {
        prefix: Vec<String>,
    },
    Invocation {
        tool: String,
        arguments: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionRule {
    pub decision: PermissionDecision,
    #[serde(flatten)]
    pub matcher: PermissionMatcher,
}

/// Persistent policy retained for one public role execution.
///
/// Deny rules remain immutable ceilings. Persistent Ask and Allow rules are
/// deliberately absent so that validated process grants are the only positive
/// authorization surface during `role-run`.
#[derive(Debug, Clone, PartialEq)]
pub struct RoleRunPermissionPolicy {
    pub legacy_rules: Vec<crate::config::ApprovalRule>,
    pub rules: Vec<PermissionRule>,
}

pub fn role_run_permission_policy(
    legacy_rules: &[crate::config::ApprovalRule],
    rules: &[PermissionRule],
) -> RoleRunPermissionPolicy {
    RoleRunPermissionPolicy {
        legacy_rules: legacy_rules
            .iter()
            .filter(|rule| legacy_decision(rule) == PermissionDecision::Deny)
            .cloned()
            .collect(),
        rules: rules
            .iter()
            .filter(|rule| rule.decision == PermissionDecision::Deny)
            .cloned()
            .collect(),
    }
}

impl PermissionRule {
    pub fn validate(&self) -> Result<(), PermissionError> {
        self.matcher.validate()
    }
}

impl PermissionMatcher {
    pub fn validate(&self) -> Result<(), PermissionError> {
        match self {
            Self::Tool { tool } => validate_tool_name(tool),
            Self::Path { tool, path_pattern } => {
                validate_tool_name(tool)?;
                validate_path_pattern(path_pattern)
            }
            Self::CommandPrefix { prefix } => validate_command_prefix(prefix),
            Self::Invocation { tool, arguments } => {
                validate_tool_name(tool)?;
                if arguments.is_object() && arguments.to_string().len() <= MAX_COMMAND_BYTES {
                    Ok(())
                } else {
                    Err(PermissionError::UnsafeCommand)
                }
            }
        }
    }

    pub fn safe_label(&self) -> String {
        match self {
            Self::Tool { tool } => tool.clone(),
            Self::Path { tool, path_pattern } => format!("{tool} · {path_pattern}"),
            Self::CommandPrefix { prefix } => {
                let shown = prefix.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
                format!(
                    "run_command · {shown}{}",
                    if prefix.len() > 2 { " …" } else { "" }
                )
            }
            Self::Invocation { tool, .. } => format!("{tool} · exact invocation"),
        }
    }

    fn matches(&self, tool: &str, arguments: &serde_json::Value) -> MatchResult {
        match self {
            Self::Tool { tool: expected } => MatchResult::from_bool(expected == tool),
            Self::Path {
                tool: expected,
                path_pattern,
            } => {
                if expected != tool {
                    return MatchResult::No;
                }
                let Ok(pattern) = Pattern::new(path_pattern) else {
                    return MatchResult::No;
                };
                MatchResult::from_bool(candidate_paths(arguments).any(|path| pattern.matches(path)))
            }
            Self::CommandPrefix { prefix } => {
                if tool != "run_command" {
                    return MatchResult::No;
                }
                let Some(command) = arguments.get("command").and_then(serde_json::Value::as_str)
                else {
                    return MatchResult::UnsafeCommand;
                };
                match parse_simple_command(command) {
                    Ok(tokens) => MatchResult::from_bool(tokens.starts_with(prefix)),
                    Err(_) => MatchResult::UnsafeCommand,
                }
            }
            Self::Invocation {
                tool: expected,
                arguments: expected_arguments,
            } => MatchResult::from_bool(
                expected == tool
                    && *expected_arguments == normalize_invocation_arguments(tool, arguments),
            ),
        }
    }

    fn subsumes(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Tool { tool: left }, Self::Tool { tool: right }) => left == right,
            (
                Self::Path {
                    tool: left_tool,
                    path_pattern: left_path,
                },
                Self::Path {
                    tool: right_tool,
                    path_pattern: right_path,
                },
            ) => left_tool == right_tool && left_path == right_path,
            (Self::CommandPrefix { prefix: left }, Self::CommandPrefix { prefix: right }) => {
                right.starts_with(left)
            }
            (
                Self::Invocation {
                    tool: left_tool,
                    arguments: left_arguments,
                },
                Self::Invocation {
                    tool: right_tool,
                    arguments: right_arguments,
                },
            ) => left_tool == right_tool && left_arguments == right_arguments,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum PermissionError {
    #[error("permission tool names must be 1-128 ASCII letters, digits, or underscores")]
    InvalidTool,
    #[error("permission path patterns must be bounded canonical absolute globs")]
    InvalidPathPattern,
    #[error("command prefixes must contain 1-8 bounded simple tokens")]
    InvalidCommandPrefix,
    #[error("shell command uses syntax that requires explicit approval")]
    UnsafeCommand,
    #[error("too many process session grants")]
    TooManySessionGrants,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchResult {
    Yes,
    No,
    UnsafeCommand,
}

impl MatchResult {
    fn from_bool(value: bool) -> Self {
        if value {
            Self::Yes
        } else {
            Self::No
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionOutcome {
    Allow,
    Ask,
    Deny,
    NoMatch,
}

/// Value-free provenance captured by the same evaluation that chooses an outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicySource {
    LegacyRule,
    PermissionRule,
    SessionGrant,
    OutboundApprovalGate,
    NoMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatcherCategory {
    Tool,
    Path,
    CommandPrefix,
    ExactInvocation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionEvaluation {
    pub outcome: PermissionOutcome,
    pub source: PolicySource,
    /// Zero-based index in the source collection at evaluation time.
    pub index: Option<usize>,
    pub matcher: Option<MatcherCategory>,
}

fn matcher_category(matcher: &PermissionMatcher) -> MatcherCategory {
    match matcher {
        PermissionMatcher::Tool { .. } => MatcherCategory::Tool,
        PermissionMatcher::Path { .. } => MatcherCategory::Path,
        PermissionMatcher::CommandPrefix { .. } => MatcherCategory::CommandPrefix,
        PermissionMatcher::Invocation { .. } => MatcherCategory::ExactInvocation,
    }
}

fn legacy_category(rule: &crate::config::ApprovalRule) -> MatcherCategory {
    match rule {
        crate::config::ApprovalRule::AllowTool(_) | crate::config::ApprovalRule::DenyTool(_) => {
            MatcherCategory::Tool
        }
        crate::config::ApprovalRule::AllowPath { .. }
        | crate::config::ApprovalRule::DenyPath { .. } => MatcherCategory::Path,
    }
}

pub fn evaluate(
    tool: &str,
    arguments: &serde_json::Value,
    legacy_rules: &[crate::config::ApprovalRule],
    rules: &[PermissionRule],
    session_grants: &[PermissionMatcher],
) -> PermissionOutcome {
    evaluate_explained(tool, arguments, legacy_rules, rules, session_grants).outcome
}

pub fn evaluate_explained(
    tool: &str,
    arguments: &serde_json::Value,
    legacy_rules: &[crate::config::ApprovalRule],
    rules: &[PermissionRule],
    session_grants: &[PermissionMatcher],
) -> PermissionEvaluation {
    if let Some(index) = legacy_rules.iter().position(|rule| {
        legacy_decision(rule) == PermissionDecision::Deny && legacy_matches(rule, tool, arguments)
    }) {
        return PermissionEvaluation {
            outcome: PermissionOutcome::Deny,
            source: PolicySource::LegacyRule,
            index: Some(index),
            matcher: Some(legacy_category(&legacy_rules[index])),
        };
    }
    if let Some(index) = rules.iter().position(|rule| {
        rule.decision == PermissionDecision::Deny
            && rule.matcher.matches(tool, arguments) == MatchResult::Yes
    }) {
        return PermissionEvaluation {
            outcome: PermissionOutcome::Deny,
            source: PolicySource::PermissionRule,
            index: Some(index),
            matcher: Some(matcher_category(&rules[index].matcher)),
        };
    }

    // External communication is never authorized by a generic tool matcher,
    // legacy allow rule, broad session grant, network grant, Yolo, or
    // AutoReview. The email manager consumes its own exact one-use approval.
    if crate::email::is_outbound_tool(tool) {
        return PermissionEvaluation {
            outcome: PermissionOutcome::NoMatch,
            source: PolicySource::OutboundApprovalGate,
            index: None,
            matcher: None,
        };
    }

    if let Some(index) = rules.iter().position(|rule| {
        rule.decision == PermissionDecision::Ask
            && rule.matcher.matches(tool, arguments) == MatchResult::Yes
    }) {
        return PermissionEvaluation {
            outcome: PermissionOutcome::Ask,
            source: PolicySource::PermissionRule,
            index: Some(index),
            matcher: Some(matcher_category(&rules[index].matcher)),
        };
    }

    if let Some(index) = session_grants
        .iter()
        .position(|grant| grant.matches(tool, arguments) == MatchResult::Yes)
    {
        return PermissionEvaluation {
            outcome: PermissionOutcome::Allow,
            source: PolicySource::SessionGrant,
            index: Some(index),
            matcher: Some(matcher_category(&session_grants[index])),
        };
    }

    if let Some(index) = legacy_rules.iter().position(|rule| {
        legacy_decision(rule) == PermissionDecision::Allow && legacy_matches(rule, tool, arguments)
    }) {
        return PermissionEvaluation {
            outcome: PermissionOutcome::Allow,
            source: PolicySource::LegacyRule,
            index: Some(index),
            matcher: Some(legacy_category(&legacy_rules[index])),
        };
    }
    if let Some(index) = rules.iter().position(|rule| {
        rule.decision == PermissionDecision::Allow
            && rule.matcher.matches(tool, arguments) == MatchResult::Yes
    }) {
        PermissionEvaluation {
            outcome: PermissionOutcome::Allow,
            source: PolicySource::PermissionRule,
            index: Some(index),
            matcher: Some(matcher_category(&rules[index].matcher)),
        }
    } else {
        PermissionEvaluation {
            outcome: PermissionOutcome::NoMatch,
            source: PolicySource::NoMatch,
            index: None,
            matcher: None,
        }
    }
}

pub fn auto_review_safe_invocation(tool: &str, arguments: &serde_json::Value) -> bool {
    use crate::agent::registry::{ToolCapability, ToolHandler, ToolRegistry};

    if host_authorized_workflow_shape(tool, arguments) {
        return true;
    }
    if tool == "preview_automation_workflow"
        && arguments.get("depth").and_then(serde_json::Value::as_str) != Some("offline")
    {
        return false;
    }

    ToolRegistry::builtins().get(tool).is_some_and(|spec| {
        tool != "apply_automation_setup"
            && tool != "disable_automation_workflow"
            && !(tool == "correct_automation_workflow"
                && arguments.get("accepted_correction_digest").is_some())
            && !matches!(
                spec.handler,
                ToolHandler::Browser | ToolHandler::ActivateSkill
            )
            && !spec.capabilities.iter().any(|capability| {
                matches!(
                    capability,
                    ToolCapability::Network
                        | ToolCapability::Privilege
                        | ToolCapability::ExternalCommunication
                )
            })
    })
}

/// These invocation shapes cannot widen authority on their own. Codex-team
/// remains responsible for proving that the named workflow is already approved
/// or that a correction is monotonic tightening before any effect occurs.
pub fn host_authorized_workflow_shape(tool: &str, arguments: &serde_json::Value) -> bool {
    match tool {
        "run_automation_workflow" => {
            arguments
                .get("workflow_id")
                .and_then(serde_json::Value::as_str)
                .is_some()
                && arguments.get("draft").is_none()
                && arguments.get("accepted_plan_digest").is_none()
        }
        "correct_automation_workflow" => {
            arguments.get("draft").is_some()
                && arguments.get("correction").is_some()
                && arguments.get("accepted_correction_digest").is_none()
        }
        _ => false,
    }
}

#[cfg(test)]
pub fn auto_review_safe_command(command: &str) -> bool {
    parse_closed_read_pipeline(command)
        .is_some_and(|pipeline| pipeline.iter().all(|stage| auto_review_safe_stage(stage)))
}

#[cfg(test)]
fn auto_review_safe_stage(words: &[String]) -> bool {
    let Some(program) = words.first() else {
        return false;
    };
    let path = std::path::Path::new(program);
    let Some(program) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if !auto_review_program_is_resolved_safely(path, program) {
        return false;
    }
    let arguments = &words[1..];
    match program {
        "find" => auto_review_find_is_read_only(arguments),
        "rg" => !arguments.iter().any(|argument| {
            matches!(argument.as_str(), "--pre" | "--pre-glob")
                || argument.starts_with("--pre=")
                || argument.starts_with("--pre-glob=")
        }),
        "diff" => !arguments.iter().any(|argument| {
            matches!(argument.as_str(), "-o" | "--output") || argument.starts_with("--output=")
        }),
        "sort" => auto_review_sort_writes_stdout_only(arguments),
        "basename" | "cat" | "cmp" | "comm" | "cut" | "df" | "dirname" | "du" | "echo"
        | "false" | "grep" | "head" | "id" | "jq" | "lsof" | "ls" | "md5" | "od" | "pgrep"
        | "printf" | "ps" | "pwd" | "readlink" | "realpath" | "shasum" | "strings" | "sw_vers"
        | "tail" | "tr" | "true" | "uname" | "wc" | "whoami" => true,
        _ => false,
    }
}

#[cfg(test)]
fn auto_review_find_is_read_only(arguments: &[String]) -> bool {
    !arguments.iter().any(|argument| {
        matches!(
            argument.as_str(),
            "-delete"
                | "-exec"
                | "-execdir"
                | "-fls"
                | "-fprint"
                | "-fprint0"
                | "-fprintf"
                | "-ok"
                | "-okdir"
        )
    })
}

#[cfg(test)]
fn auto_review_sort_writes_stdout_only(arguments: &[String]) -> bool {
    !arguments.iter().any(|argument| {
        argument == "-o"
            || (argument.starts_with("-o") && !argument.starts_with("--"))
            || matches!(argument.as_str(), "--output" | "--compress-program")
            || argument.starts_with("--output=")
            || argument.starts_with("--compress-program=")
    })
}

#[cfg(test)]
fn auto_review_program_is_resolved_safely(path: &std::path::Path, program: &str) -> bool {
    if path.components().count() > 1 {
        return path.is_absolute() && crate::jobs::trusted_executable(path);
    }
    if matches!(program, "echo" | "false" | "printf" | "pwd" | "true") {
        return true;
    }
    let Some(search_path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&search_path).find_map(|directory| {
        let candidate = directory.join(program);
        candidate
            .is_file()
            .then(|| directory.is_absolute() && crate::jobs::trusted_executable(&candidate))
    }) == Some(true)
}

#[cfg(test)]
fn parse_closed_read_pipeline(command: &str) -> Option<Vec<Vec<String>>> {
    parse_closed_pipeline(command, false)
}

#[cfg(test)]
fn parse_closed_pipeline(command: &str, allow_path_globs: bool) -> Option<Vec<Vec<String>>> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }

    if command.is_empty() || command.len() > MAX_COMMAND_BYTES {
        return None;
    }
    let mut pipeline = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote = Quote::None;
    for character in command.chars() {
        match quote {
            Quote::None if character.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            Quote::None if character == '\'' => {
                quote = Quote::Single;
                started = true;
            }
            Quote::None if character == '"' => {
                quote = Quote::Double;
                started = true;
            }
            Quote::None if character == '|' => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
                if words.is_empty() || pipeline.len() >= MAX_COMMAND_TOKENS {
                    return None;
                }
                pipeline.push(std::mem::take(&mut words));
            }
            Quote::None
                if character.is_control()
                    || matches!(
                        character,
                        ';' | '&'
                            | '<'
                            | '>'
                            | '$'
                            | '`'
                            | '\\'
                            | '('
                            | ')'
                            | '{'
                            | '}'
                            | '~'
                            | '#'
                    ) =>
            {
                return None;
            }
            Quote::None if !allow_path_globs && matches!(character, '[' | ']' | '*' | '?') => {
                return None;
            }
            Quote::None => {
                word.push(character);
                started = true;
            }
            Quote::Single if character == '\'' => quote = Quote::None,
            Quote::Single => {
                if character.is_control() {
                    return None;
                }
                word.push(character);
            }
            Quote::Double if character == '"' => quote = Quote::None,
            Quote::Double if character.is_control() || matches!(character, '$' | '`' | '\\') => {
                return None;
            }
            Quote::Double => word.push(character),
        }
        let token_count = pipeline.iter().map(Vec::len).sum::<usize>() + words.len();
        if token_count > MAX_COMMAND_TOKENS || word.len() > MAX_PREFIX_TOKEN_BYTES {
            return None;
        }
    }
    if quote != Quote::None {
        return None;
    }
    if started {
        words.push(word);
    }
    if words.is_empty() {
        return None;
    }
    pipeline.push(words);
    let token_count = pipeline.iter().map(Vec::len).sum::<usize>();
    (token_count <= MAX_COMMAND_TOKENS).then_some(pipeline)
}

fn legacy_decision(rule: &crate::config::ApprovalRule) -> PermissionDecision {
    match rule {
        crate::config::ApprovalRule::AllowTool(_)
        | crate::config::ApprovalRule::AllowPath { .. } => PermissionDecision::Allow,
        crate::config::ApprovalRule::DenyTool(_) | crate::config::ApprovalRule::DenyPath { .. } => {
            PermissionDecision::Deny
        }
    }
}

fn legacy_matches(
    rule: &crate::config::ApprovalRule,
    tool: &str,
    arguments: &serde_json::Value,
) -> bool {
    match rule {
        crate::config::ApprovalRule::AllowTool(expected)
        | crate::config::ApprovalRule::DenyTool(expected) => expected == tool,
        crate::config::ApprovalRule::AllowPath {
            tool: expected,
            path_pattern,
        }
        | crate::config::ApprovalRule::DenyPath {
            tool: expected,
            path_pattern,
        } => {
            expected == tool
                && Pattern::new(path_pattern).is_ok_and(|pattern| {
                    candidate_paths(arguments).any(|path| pattern.matches(path))
                })
        }
    }
}

fn candidate_paths(arguments: &serde_json::Value) -> impl Iterator<Item = &str> {
    ["path", "cwd", "dest"]
        .into_iter()
        .filter_map(|field| arguments.get(field).and_then(serde_json::Value::as_str))
}

pub fn derive_session_grant(
    tool: &str,
    arguments: &serde_json::Value,
) -> Result<PermissionMatcher, PermissionError> {
    if crate::email::is_outbound_tool(tool) {
        return Err(PermissionError::UnsafeCommand);
    }
    if tool == "run_command" {
        if arguments
            .get("command")
            .and_then(serde_json::Value::as_str)
            .is_none()
        {
            return Err(PermissionError::UnsafeCommand);
        }
        let matcher = PermissionMatcher::Invocation {
            tool: tool.to_string(),
            arguments: normalize_invocation_arguments(tool, arguments),
        };
        matcher.validate()?;
        return Ok(matcher);
    }
    if let Some(path) = candidate_paths(arguments).next() {
        let matcher = PermissionMatcher::Path {
            tool: tool.to_string(),
            path_pattern: Pattern::escape(path),
        };
        matcher.validate()?;
        return Ok(matcher);
    }
    let matcher = PermissionMatcher::Invocation {
        tool: tool.to_string(),
        arguments: normalize_invocation_arguments(tool, arguments),
    };
    matcher.validate()?;
    Ok(matcher)
}

fn normalize_invocation_arguments(tool: &str, arguments: &serde_json::Value) -> serde_json::Value {
    let mut normalized = arguments.clone();
    let Some(object) = normalized.as_object_mut() else {
        return serde_json::json!({});
    };
    if tool == "run_command" {
        object
            .entry("background")
            .or_insert_with(|| serde_json::Value::Bool(false));
        object
            .entry("network")
            .or_insert_with(|| serde_json::Value::Bool(false));
    } else if tool == "preview_automation_workflow" {
        // One exact-draft approval covers the progressively deeper read-only
        // and mutation previews. The draft remains part of the matcher, so the
        // grant cannot spill into a different workflow or widened revision.
        object.remove("depth");
        object.remove("correction_round");
    }
    normalized
}

#[derive(Debug, Clone, Default)]
pub struct SessionGrantStore {
    grants: Vec<PermissionMatcher>,
}

impl SessionGrantStore {
    pub fn grants(&self) -> &[PermissionMatcher] {
        &self.grants
    }

    pub fn snapshot(&self) -> Vec<PermissionMatcher> {
        self.grants.clone()
    }

    pub fn add(&mut self, grant: PermissionMatcher) -> Result<(), PermissionError> {
        grant.validate()?;
        if self.grants.iter().any(|existing| existing.subsumes(&grant)) {
            return Ok(());
        }
        self.grants.retain(|existing| !grant.subsumes(existing));
        if self.grants.len() >= MAX_SESSION_GRANTS {
            return Err(PermissionError::TooManySessionGrants);
        }
        self.grants.push(grant);
        Ok(())
    }

    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.grants.len() {
            return false;
        }
        self.grants.remove(index);
        true
    }
}

pub fn parse_simple_command(command: &str) -> Result<Vec<String>, PermissionError> {
    if command.is_empty() || command.len() > MAX_COMMAND_BYTES {
        return Err(PermissionError::UnsafeCommand);
    }
    #[derive(Clone, Copy)]
    enum Quote {
        None,
        Single,
        Double,
    }
    let mut quote = Quote::None;
    let mut tokens = Vec::new();
    let mut token = String::new();
    for character in command.chars() {
        if character.is_control()
            || matches!(character, ';' | '|' | '&' | '<' | '>' | '$' | '`' | '\\')
            || (matches!(quote, Quote::None) && matches!(character, '*' | '?' | '[' | ']' | '#'))
        {
            return Err(PermissionError::UnsafeCommand);
        }
        match (quote, character) {
            (Quote::None, '\'') => quote = Quote::Single,
            (Quote::None, '"') => quote = Quote::Double,
            (Quote::Single, '\'') | (Quote::Double, '"') => quote = Quote::None,
            (Quote::None, value) if value.is_whitespace() => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                    if tokens.len() > MAX_COMMAND_TOKENS {
                        return Err(PermissionError::UnsafeCommand);
                    }
                }
            }
            (_, value) => token.push(value),
        }
    }
    if !matches!(quote, Quote::None) {
        return Err(PermissionError::UnsafeCommand);
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    if tokens.is_empty() || tokens.len() > MAX_COMMAND_TOKENS {
        return Err(PermissionError::UnsafeCommand);
    }
    Ok(tokens)
}

fn validate_tool_name(tool: &str) -> Result<(), PermissionError> {
    let valid = !tool.is_empty()
        && tool.len() <= MAX_TOOL_NAME_BYTES
        && tool
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    if valid {
        Ok(())
    } else {
        Err(PermissionError::InvalidTool)
    }
}

fn validate_path_pattern(pattern: &str) -> Result<(), PermissionError> {
    let valid = pattern.starts_with('/')
        && pattern.len() <= MAX_PATH_PATTERN_BYTES
        && !pattern.chars().any(char::is_control)
        && !pattern.split('/').any(|component| component == "..")
        && Pattern::new(pattern).is_ok();
    if valid {
        Ok(())
    } else {
        Err(PermissionError::InvalidPathPattern)
    }
}

fn validate_command_prefix(prefix: &[String]) -> Result<(), PermissionError> {
    let valid = !prefix.is_empty()
        && prefix.len() <= MAX_PREFIX_TOKENS
        && prefix.iter().all(|token| {
            !token.is_empty()
                && token.len() <= MAX_PREFIX_TOKEN_BYTES
                && !token.chars().any(|character| {
                    character.is_control()
                        || character.is_whitespace()
                        || matches!(
                            character,
                            ';' | '|' | '&' | '<' | '>' | '$' | '`' | '\\' | '\'' | '"'
                        )
                })
        });
    if valid {
        Ok(())
    } else {
        Err(PermissionError::InvalidCommandPrefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(decision: PermissionDecision, matcher: PermissionMatcher) -> PermissionRule {
        PermissionRule { decision, matcher }
    }

    #[test]
    fn agy295_explanations_keep_original_precedence_and_source_indices() {
        let arguments = serde_json::json!({"path":"/workspace/private/value"});
        let legacy = [
            crate::config::ApprovalRule::AllowTool("write_file".into()),
            crate::config::ApprovalRule::DenyPath {
                tool: "write_file".into(),
                path_pattern: "/workspace/private/**".into(),
            },
        ];
        let rules = [
            rule(
                PermissionDecision::Allow,
                PermissionMatcher::Tool {
                    tool: "write_file".into(),
                },
            ),
            rule(
                PermissionDecision::Ask,
                PermissionMatcher::Tool {
                    tool: "write_file".into(),
                },
            ),
            rule(
                PermissionDecision::Deny,
                PermissionMatcher::Path {
                    tool: "write_file".into(),
                    path_pattern: "/workspace/private/**".into(),
                },
            ),
        ];
        let grants = [PermissionMatcher::Invocation {
            tool: "write_file".into(),
            arguments: arguments.clone(),
        }];
        let cases = [
            (
                &legacy[..],
                &rules[..],
                &grants[..],
                PermissionOutcome::Deny,
                PolicySource::LegacyRule,
                Some(1),
                Some(MatcherCategory::Path),
            ),
            (
                &legacy[..1],
                &rules[..],
                &grants[..],
                PermissionOutcome::Deny,
                PolicySource::PermissionRule,
                Some(2),
                Some(MatcherCategory::Path),
            ),
            (
                &legacy[..1],
                &rules[..2],
                &grants[..],
                PermissionOutcome::Ask,
                PolicySource::PermissionRule,
                Some(1),
                Some(MatcherCategory::Tool),
            ),
            (
                &legacy[..1],
                &rules[..1],
                &grants[..],
                PermissionOutcome::Allow,
                PolicySource::SessionGrant,
                Some(0),
                Some(MatcherCategory::ExactInvocation),
            ),
            (
                &legacy[..1],
                &rules[..1],
                &[][..],
                PermissionOutcome::Allow,
                PolicySource::LegacyRule,
                Some(0),
                Some(MatcherCategory::Tool),
            ),
            (
                &[][..],
                &rules[..1],
                &[][..],
                PermissionOutcome::Allow,
                PolicySource::PermissionRule,
                Some(0),
                Some(MatcherCategory::Tool),
            ),
            (
                &[][..],
                &[][..],
                &[][..],
                PermissionOutcome::NoMatch,
                PolicySource::NoMatch,
                None,
                None,
            ),
        ];
        for (legacy, rules, grants, outcome, source, index, matcher) in cases {
            let explained = evaluate_explained("write_file", &arguments, legacy, rules, grants);
            assert_eq!(
                explained,
                PermissionEvaluation {
                    outcome,
                    source,
                    index,
                    matcher
                }
            );
            assert_eq!(
                evaluate("write_file", &arguments, legacy, rules, grants),
                outcome
            );
            assert!(!format!("{explained:?}").contains("/workspace/private"));
        }
    }

    #[test]
    fn agy295_explanation_preserves_outbound_and_unsafe_command_gates() {
        let allow = [rule(
            PermissionDecision::Allow,
            PermissionMatcher::Tool {
                tool: "email_send".into(),
            },
        )];
        let grants = [PermissionMatcher::Tool {
            tool: "email_send".into(),
        }];
        let evaluation =
            evaluate_explained("email_send", &serde_json::json!({}), &[], &allow, &grants);
        assert_eq!(evaluation.outcome, PermissionOutcome::NoMatch);
        assert_eq!(evaluation.source, PolicySource::OutboundApprovalGate);
        assert_eq!(evaluation.matcher, None);

        let command = [PermissionMatcher::CommandPrefix {
            prefix: vec!["echo".into()],
        }];
        let safe = evaluate_explained(
            "run_command",
            &serde_json::json!({"command":"echo secret-value"}),
            &[],
            &[],
            &command,
        );
        assert_eq!(safe.matcher, Some(MatcherCategory::CommandPrefix));
        let unsafe_command = evaluate_explained(
            "run_command",
            &serde_json::json!({"command":"echo secret-value; cat private-value"}),
            &[],
            &[],
            &command,
        );
        assert_eq!(unsafe_command.outcome, PermissionOutcome::NoMatch);
        assert!(!format!("{safe:?} {unsafe_command:?}").contains("secret-value"));
    }

    #[test]
    fn role_run_policy_keeps_only_modern_and_legacy_deny_ceilings() {
        let legacy_allow = crate::config::ApprovalRule::AllowTool("write_file".into());
        let legacy_deny = crate::config::ApprovalRule::DenyPath {
            tool: "write_file".into(),
            path_pattern: "/workspace/protected/**".into(),
        };
        let modern_allow = rule(
            PermissionDecision::Allow,
            PermissionMatcher::Tool {
                tool: "write_file".into(),
            },
        );
        let modern_ask = rule(
            PermissionDecision::Ask,
            PermissionMatcher::Path {
                tool: "write_file".into(),
                path_pattern: "/workspace/src/**".into(),
            },
        );
        let modern_deny = rule(
            PermissionDecision::Deny,
            PermissionMatcher::Path {
                tool: "write_file".into(),
                path_pattern: "/workspace/secret/**".into(),
            },
        );

        let policy = role_run_permission_policy(
            &[legacy_allow, legacy_deny.clone()],
            &[modern_allow, modern_ask, modern_deny.clone()],
        );

        assert_eq!(policy.legacy_rules, vec![legacy_deny]);
        assert_eq!(policy.rules, vec![modern_deny]);

        let grant = PermissionMatcher::Tool {
            tool: "write_file".into(),
        };
        assert_eq!(
            evaluate(
                "write_file",
                &serde_json::json!({"path":"/workspace/src/lib.rs"}),
                &policy.legacy_rules,
                &policy.rules,
                std::slice::from_ref(&grant),
            ),
            PermissionOutcome::Allow,
            "the process grant is the sole positive authority"
        );
        assert_eq!(
            evaluate(
                "write_file",
                &serde_json::json!({"path":"/workspace/protected/key"}),
                &policy.legacy_rules,
                &policy.rules,
                std::slice::from_ref(&grant),
            ),
            PermissionOutcome::Deny,
            "legacy Deny remains above a process grant"
        );
        assert_eq!(
            evaluate(
                "write_file",
                &serde_json::json!({"path":"/workspace/secret/key"}),
                &policy.legacy_rules,
                &policy.rules,
                std::slice::from_ref(&grant),
            ),
            PermissionOutcome::Deny,
            "modern Deny remains above a process grant"
        );
        assert_eq!(
            evaluate(
                "write_file",
                &serde_json::json!({"path":"/workspace/src/lib.rs"}),
                &policy.legacy_rules,
                &policy.rules,
                &[],
            ),
            PermissionOutcome::NoMatch,
            "discarded persistent Allow and Ask rules cannot authorize or prompt"
        );
    }

    #[test]
    fn outbound_email_ignores_every_generic_allow_surface() {
        let arguments = serde_json::json!({"draft_ref":"draft_00000000000000000000000000000000"});
        assert_eq!(
            evaluate(
                "email_send",
                &arguments,
                &[crate::config::ApprovalRule::AllowTool("email_send".into())],
                &[rule(
                    PermissionDecision::Allow,
                    PermissionMatcher::Tool {
                        tool: "email_send".into(),
                    },
                )],
                &[PermissionMatcher::Tool {
                    tool: "email_send".into(),
                }],
            ),
            PermissionOutcome::NoMatch
        );
        assert!(derive_session_grant("email_send", &arguments).is_err());
        assert_eq!(
            evaluate(
                "email_send",
                &arguments,
                &[crate::config::ApprovalRule::DenyTool("email_send".into())],
                &[],
                &[],
            ),
            PermissionOutcome::Deny
        );
    }

    #[test]
    fn fixed_precedence_is_deny_then_explicit_ask_then_grant_then_allow() {
        let arguments = serde_json::json!({"path":"/workspace/src/lib.rs"});
        let allow = rule(
            PermissionDecision::Allow,
            PermissionMatcher::Tool {
                tool: "write_file".into(),
            },
        );
        let ask = rule(
            PermissionDecision::Ask,
            PermissionMatcher::Path {
                tool: "write_file".into(),
                path_pattern: "/workspace/src/**".into(),
            },
        );
        let deny = rule(
            PermissionDecision::Deny,
            PermissionMatcher::Path {
                tool: "write_file".into(),
                path_pattern: "/workspace/src/lib.rs".into(),
            },
        );
        let grant = PermissionMatcher::Path {
            tool: "write_file".into(),
            path_pattern: "/workspace/src/lib.rs".into(),
        };

        assert_eq!(
            evaluate(
                "write_file",
                &arguments,
                &[],
                &[allow.clone(), ask.clone()],
                &[]
            ),
            PermissionOutcome::Ask
        );
        assert_eq!(
            evaluate(
                "write_file",
                &arguments,
                &[],
                &[allow, ask],
                std::slice::from_ref(&grant)
            ),
            PermissionOutcome::Ask
        );
        assert_eq!(
            evaluate("write_file", &arguments, &[], &[deny], &[grant]),
            PermissionOutcome::Deny
        );
    }

    #[test]
    fn command_rules_match_simple_prefixes_while_modes_classify_complex_shell() {
        let allow = rule(
            PermissionDecision::Allow,
            PermissionMatcher::CommandPrefix {
                prefix: vec!["cargo".into(), "test".into()],
            },
        );
        let broad = crate::config::ApprovalRule::AllowTool("run_command".into());
        assert_eq!(
            evaluate(
                "run_command",
                &serde_json::json!({"command":"cargo test --all"}),
                &[],
                std::slice::from_ref(&allow),
                &[],
            ),
            PermissionOutcome::Allow
        );
        for command in [
            "cargo test && rm -rf build",
            "cargo test > out",
            "cargo $(echo test)",
            "cargo test\nrm build",
        ] {
            assert_eq!(
                evaluate(
                    "run_command",
                    &serde_json::json!({"command":command}),
                    std::slice::from_ref(&broad),
                    std::slice::from_ref(&allow),
                    &[PermissionMatcher::Tool {
                        tool: "run_command".into()
                    }],
                ),
                PermissionOutcome::Allow,
                "{command}"
            );
        }
    }

    #[test]
    fn legacy_closed_read_parser_remains_conservative() {
        for command in [
            "pwd",
            "ls -la",
            "rg network_requested src",
            "find src -maxdepth 3 -type f | sort",
            "find . -name '*.rs' -print",
            "sort Cargo.toml",
            "ps auxww | grep '[r]sync'",
            "cat Cargo.toml | head -n 20",
            "/usr/bin/wc -l Cargo.toml",
        ] {
            assert!(auto_review_safe_command(command), "{command}");
        }
        for command in [
            "ssh bsd true",
            "git status --short",
            "cargo test",
            "rg --pre ./decode needle .",
            "diff --output=copy old new",
            "find . -exec touch marker ;",
            "find . -exec touch marker ';'",
            "find . -execdir touch marker ';'",
            "find . -ok touch marker ';'",
            "find . -okdir touch marker ';'",
            "find . -delete",
            "find . -fls copy",
            "find . -fprint copy",
            "find . -fprint0 copy",
            "find . -fprintf copy '%p\\n'",
            "sort -o copy Cargo.toml",
            "sort -ocopy Cargo.toml",
            "sort --output copy Cargo.toml",
            "sort --output=copy Cargo.toml",
            "sort --compress-program gzip Cargo.toml",
            "sort --compress-program=gzip Cargo.toml",
            "cat Cargo.toml > copy",
            "cat $(touch marker)",
            "cat Cargo.toml | tee copy",
            "pwd && touch marker",
            "./rg needle src",
            "env rg needle src",
            "unknown-reader file",
        ] {
            assert!(!auto_review_safe_command(command), "{command}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn auto_review_rejects_shadowable_executables_outside_trusted_roots() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let fake = root.path().join("rg");
        std::fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!auto_review_program_is_resolved_safely(&fake, "rg"));
        assert!(auto_review_program_is_resolved_safely(
            std::path::Path::new("/usr/bin/grep"),
            "grep"
        ));
    }

    #[test]
    fn auto_review_allows_prepared_local_work_and_keeps_boundary_tools_gated() {
        for (tool, arguments) in [
            (
                "spawn_agent",
                serde_json::json!({"task":"edit", "tools":["write_file"]}),
            ),
            (
                "task",
                serde_json::json!({"subagent_name":"general", "prompt":"edit"}),
            ),
            (
                "run_command",
                serde_json::json!({"command":"cargo test","background":true}),
            ),
            (
                "write_file",
                serde_json::json!({"path":"out","content":"ok"}),
            ),
            ("await_job", serde_json::json!({"job_id":"job-1"})),
        ] {
            assert!(auto_review_safe_invocation(tool, &arguments), "{tool}");
        }
        for tool in [
            "web_search",
            "browser",
            "activate_skill",
            "email_send",
            "run_privileged",
            "missing",
        ] {
            assert!(
                !auto_review_safe_invocation(tool, &serde_json::json!({})),
                "{tool}"
            );
        }
    }

    #[test]
    fn workflow_auto_review_only_admits_host_revalidated_non_widening_shapes() {
        assert!(auto_review_safe_invocation(
            "run_automation_workflow",
            &serde_json::json!({"workflow_id":"seedbox_archive"})
        ));
        assert!(!auto_review_safe_invocation(
            "run_automation_workflow",
            &serde_json::json!({
                "workflow_id":"seedbox_archive",
                "draft":{},
                "accepted_plan_digest":"a".repeat(64)
            })
        ));
        assert!(auto_review_safe_invocation(
            "correct_automation_workflow",
            &serde_json::json!({"draft":{},"correction":{}})
        ));
        assert!(!auto_review_safe_invocation(
            "correct_automation_workflow",
            &serde_json::json!({
                "draft":{},
                "correction":{},
                "accepted_correction_digest":"a".repeat(64)
            })
        ));
        assert!(auto_review_safe_invocation(
            "preview_automation_workflow",
            &serde_json::json!({"draft":{},"depth":"offline"})
        ));
        assert!(!auto_review_safe_invocation(
            "preview_automation_workflow",
            &serde_json::json!({"draft":{},"depth":"mutation_preview"})
        ));
        assert!(!auto_review_safe_invocation(
            "disable_automation_workflow",
            &serde_json::json!({
                "workflow_id":"seedbox_archive",
                "accepted_workflow_digest":"a".repeat(64)
            })
        ));
    }

    #[test]
    fn one_exact_workflow_network_grant_covers_both_probe_depths() {
        let read_only = serde_json::json!({
            "draft":{"schema":"codex-team.workflow_draft.v1","workflow_id":"archive"},
            "depth":"read_only_probe",
            "correction_round":0
        });
        let mutation = serde_json::json!({
            "draft":{"schema":"codex-team.workflow_draft.v1","workflow_id":"archive"},
            "depth":"mutation_preview",
            "correction_round":1
        });
        let changed = serde_json::json!({
            "draft":{"schema":"codex-team.workflow_draft.v1","workflow_id":"other"},
            "depth":"mutation_preview",
            "correction_round":0
        });
        let grant = derive_session_grant("preview_automation_workflow", &read_only)
            .expect("exact workflow grant");
        assert_eq!(
            grant.matches("preview_automation_workflow", &mutation),
            MatchResult::Yes
        );
        assert_eq!(
            grant.matches("preview_automation_workflow", &changed),
            MatchResult::No
        );
    }

    #[test]
    fn shell_shape_does_not_override_explicit_permission_rules() {
        let command = serde_json::json!({"command":"ps auxww | grep '[r]sync'"});
        assert_eq!(
            evaluate("run_command", &command, &[], &[], &[]),
            PermissionOutcome::NoMatch
        );
        let broad_allow = crate::config::ApprovalRule::AllowTool("run_command".into());
        assert_eq!(
            evaluate("run_command", &command, &[broad_allow], &[], &[]),
            PermissionOutcome::Allow
        );
        assert_eq!(
            evaluate(
                "run_command",
                &serde_json::json!({"command":"cat Cargo.toml | tee copy"}),
                &[crate::config::ApprovalRule::AllowTool("run_command".into())],
                &[],
                &[],
            ),
            PermissionOutcome::Allow
        );
    }

    #[test]
    fn auto_review_preserves_explicit_rule_precedence() {
        let arguments = serde_json::json!({"command":"rg needle src"});
        let ask = PermissionRule {
            decision: PermissionDecision::Ask,
            matcher: PermissionMatcher::Tool {
                tool: "run_command".into(),
            },
        };
        let deny = PermissionRule {
            decision: PermissionDecision::Deny,
            matcher: PermissionMatcher::Tool {
                tool: "run_command".into(),
            },
        };
        assert_eq!(
            evaluate("run_command", &arguments, &[], &[ask], &[]),
            PermissionOutcome::Ask
        );
        assert_eq!(
            evaluate("run_command", &arguments, &[], &[deny], &[]),
            PermissionOutcome::Deny
        );

        let spawn = serde_json::json!({"task":"inspect", "tools":["read_file"]});
        let ask_spawn = PermissionRule {
            decision: PermissionDecision::Ask,
            matcher: PermissionMatcher::Tool {
                tool: "spawn_agent".into(),
            },
        };
        let deny_spawn = PermissionRule {
            decision: PermissionDecision::Deny,
            matcher: PermissionMatcher::Tool {
                tool: "spawn_agent".into(),
            },
        };
        assert_eq!(
            evaluate("spawn_agent", &spawn, &[], &[ask_spawn], &[]),
            PermissionOutcome::Ask
        );
        assert_eq!(
            evaluate("spawn_agent", &spawn, &[], &[deny_spawn], &[]),
            PermissionOutcome::Deny
        );
    }

    #[test]
    fn path_rules_match_only_prepared_absolute_paths() {
        let path_rule = rule(
            PermissionDecision::Deny,
            PermissionMatcher::Path {
                tool: "write_file".into(),
                path_pattern: "/workspace/private/**".into(),
            },
        );
        assert_eq!(
            evaluate(
                "write_file",
                &serde_json::json!({"path":"/workspace/private/key"}),
                &[],
                &[path_rule],
                &[],
            ),
            PermissionOutcome::Deny
        );
        assert!(PermissionMatcher::Path {
            tool: "write_file".into(),
            path_pattern: "../private/**".into(),
        }
        .validate()
        .is_err());

        let exact_metacharacter_path = derive_session_grant(
            "write_file",
            &serde_json::json!({"path":"/workspace/src/[draft]*.rs"}),
        )
        .unwrap();
        assert_eq!(
            exact_metacharacter_path.matches(
                "write_file",
                &serde_json::json!({"path":"/workspace/src/[draft]*.rs"})
            ),
            MatchResult::Yes
        );
        assert_eq!(
            exact_metacharacter_path.matches(
                "write_file",
                &serde_json::json!({"path":"/workspace/src/d.rs"})
            ),
            MatchResult::No
        );
    }

    #[test]
    fn session_grants_are_bounded_deduplicated_and_prefix_subsuming() {
        let mut store = SessionGrantStore::default();
        store
            .add(PermissionMatcher::CommandPrefix {
                prefix: vec!["cargo".into(), "test".into(), "-p".into()],
            })
            .unwrap();
        store
            .add(PermissionMatcher::CommandPrefix {
                prefix: vec!["cargo".into(), "test".into()],
            })
            .unwrap();
        assert_eq!(store.grants().len(), 1);
        assert_eq!(store.grants()[0].safe_label(), "run_command · cargo test");
        assert!(store.remove(0));
        assert!(store.grants().is_empty());
    }

    #[test]
    fn session_grants_use_canonical_path_or_exact_normalized_invocation() {
        assert_eq!(
            derive_session_grant(
                "write_file",
                &serde_json::json!({"path":"/workspace/src/lib.rs"}),
            )
            .unwrap(),
            PermissionMatcher::Path {
                tool: "write_file".into(),
                path_pattern: "/workspace/src/lib.rs".into(),
            }
        );
        assert_eq!(
            derive_session_grant(
                "run_command",
                &serde_json::json!({"command":"cargo test --all"}),
            )
            .unwrap(),
            PermissionMatcher::Invocation {
                tool: "run_command".into(),
                arguments: serde_json::json!({
                    "command":"cargo test --all",
                    "background":false,
                    "network":false
                }),
            }
        );
        let compound = derive_session_grant(
            "run_command",
            &serde_json::json!({"command":"cargo test | sh","cwd":"/workspace"}),
        )
        .unwrap();
        assert_eq!(
            compound.matches(
                "run_command",
                &serde_json::json!({
                    "command":"cargo test | sh",
                    "cwd":"/workspace",
                    "background":false,
                    "network":false
                })
            ),
            MatchResult::Yes
        );

        let watcher = serde_json::json!({
            "label":"BSD copy",
            "host":{"type":"ssh","destination":"root@bsd","port":2222},
            "pid":55347,
            "process":{
                "type":"copy",
                "log_path":"/home/aji/copy.log",
                "completion_marker":"/bigmac/seedbox1/.copy-complete"
            },
            "interval_secs":10
        });
        let watcher_grant = derive_session_grant("watch_process", &watcher).unwrap();
        assert_eq!(
            watcher_grant.matches("watch_process", &watcher),
            MatchResult::Yes
        );
        for changed in [
            serde_json::json!({
                "label":"BSD copy",
                "host":{"type":"ssh","destination":"root@other","port":2222},
                "pid":55347,
                "process":{"type":"copy","log_path":"/home/aji/copy.log","completion_marker":"/bigmac/seedbox1/.copy-complete"},
                "interval_secs":10
            }),
            serde_json::json!({
                "label":"BSD copy",
                "host":{"type":"ssh","destination":"root@bsd","port":2222},
                "pid":55348,
                "process":{"type":"copy","log_path":"/home/aji/copy.log","completion_marker":"/bigmac/seedbox1/.copy-complete"},
                "interval_secs":10
            }),
            serde_json::json!({
                "label":"BSD copy",
                "host":{"type":"ssh","destination":"root@bsd","port":2222},
                "pid":55347,
                "process":{"type":"copy","log_path":"/home/aji/other.log","completion_marker":"/bigmac/seedbox1/.copy-complete"},
                "interval_secs":10
            }),
            serde_json::json!({
                "label":"BSD copy",
                "host":{"type":"ssh","destination":"root@bsd","port":2222},
                "pid":55347,
                "process":{"type":"copy","log_path":"/home/aji/copy.log","completion_marker":"/bigmac/seedbox1/.copy-complete"},
                "interval_secs":20
            }),
        ] {
            assert_eq!(
                watcher_grant.matches("watch_process", &changed),
                MatchResult::No
            );
        }
        assert_eq!(
            compound.matches(
                "run_command",
                &serde_json::json!({"command":"cargo test | sh","cwd":"/other"})
            ),
            MatchResult::No
        );
    }

    #[test]
    fn manager_labels_never_include_command_tails() {
        let matcher = PermissionMatcher::CommandPrefix {
            prefix: vec![
                "deploy".into(),
                "production".into(),
                "possibly-sensitive-tail".into(),
            ],
        };
        assert_eq!(matcher.safe_label(), "run_command · deploy production …");
        assert!(!matcher.safe_label().contains("possibly-sensitive-tail"));
    }
}
