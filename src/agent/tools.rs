use crate::agent::registry::{
    ToolContext, ToolExecutionResult, ToolHandler, ToolProfile, ToolRegistry,
};
use futures::StreamExt;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::LazyLock;
use std::time::Duration;
use tokio::io::AsyncReadExt;

const READ_FILE_DEFAULT_LINES: usize = 200;
const READ_FILE_MAX_LINES: usize = 500;
const READ_FILE_MAX_RESULT_BYTES: usize = 64 * 1024;
const READ_FILE_LINE_EDGE_BYTES: usize = 4 * 1024;
const PDF_MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
const PDF_MAX_PAGES: usize = 10_000;
const PDF_MAX_RANGE: usize = 20;
const PDF_TEXT_CAP_BYTES: usize = 56 * 1024;
const PDF_MAX_RESULT_BYTES: usize = 64 * 1024;
const WEB_FETCH_MAX_DECODED_BYTES: usize = 1024 * 1024;
const WEB_FETCH_MAX_RESULT_BYTES: usize = 4 * 1024;
pub const MAX_NATIVE_READ_CONCURRENCY: usize = 4;

/// Arguments must already have passed the caller's authority and workspace preparation.
#[derive(Clone)]
pub struct PreparedNativeRead {
    pub name: String,
    pub args: Value,
}

/// Execute one admitted contiguous native run, preserving provider result order.
/// The caller retains routing, hook, and approval barriers; this function never dispatches
/// an ineligible call or runs more than four calls.
pub async fn execute_native_read_run(
    calls: &[PreparedNativeRead],
    context: &ToolContext,
) -> Vec<ToolExecutionResult> {
    let registry = ToolRegistry::builtins();
    if calls.len() > MAX_NATIVE_READ_CONCURRENCY
        || calls
            .iter()
            .any(|call| !registry.supports_native_read(&call.name, context.profile))
    {
        return calls
            .iter()
            .map(|_| {
                ToolExecutionResult::Failure(
                    "Error: native read run contains an ineligible call or exceeds four calls"
                        .into(),
                )
            })
            .collect();
    }
    futures::future::join_all(calls.iter().map(|call| async {
        match tokio::time::timeout(
            Duration::from_secs(context.timeout_secs.clamp(1, 30)),
            execute_tool_with_context(&call.name, &call.args, context),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => ToolExecutionResult::Failure("Error: native read timed out".into()),
        }
    }))
    .await
}

static TODO_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)(TODO|FIXME|HACK|NOTE|XXX|BUG|OPTIMIZE)").unwrap());

/// OpenAI-compatible schemas selected by the main tool profile.
pub fn default_tool_defs() -> Vec<crate::agent::client::ToolDef> {
    crate::tool_selection::resolve_tools(
        crate::agent::registry::ToolRegistry::builtins(),
        crate::agent::registry::ToolProfile::Main,
        &crate::tool_selection::ToolSelectionPolicy::default(),
        &crate::tool_selection::ToolSelectionConstraints::default(),
    )
    .expect("built-in default tool selection must remain valid")
    .into_definitions()
}

fn resolve_subagent_route(
    config: &crate::config::CaConfig,
    requested_model: Option<&str>,
) -> (String, crate::config::ProviderConfig, String) {
    if let Some(model) = requested_model {
        if let Some(provider) = config.providers.get(model) {
            return (
                model.to_string(),
                provider.clone(),
                provider
                    .models
                    .first()
                    .cloned()
                    .unwrap_or_else(|| model.to_string()),
            );
        }
        let (provider_id, provider) = config
            .providers
            .iter()
            .next()
            .map(|(id, provider)| (id.clone(), provider.clone()))
            .unwrap_or_else(|| {
                (
                    "ollama".into(),
                    crate::config::ProviderConfig {
                        google: None,
                        tool_schema_policy: Default::default(),
                        base_url: "http://127.0.0.1:11434/v1".into(),
                        api_key: "ollama".into(),
                        protocol: crate::agent::ProviderProtocol::default(),
                        prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                        models: vec![model.to_string()],
                        timeout: crate::config::ProviderTimeoutConfig::default(),
                    },
                )
            });
        return (provider_id, provider, model.to_string());
    }

    let provider_id = config.defaults.provider.clone();
    let provider = config
        .providers
        .get(&provider_id)
        .cloned()
        .unwrap_or_else(|| crate::config::ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: "http://127.0.0.1:11434/v1".into(),
            api_key: "ollama".into(),
            protocol: crate::agent::ProviderProtocol::default(),
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["auto".into()],
            timeout: crate::config::ProviderTimeoutConfig::default(),
        });
    let model = provider
        .models
        .first()
        .cloned()
        .unwrap_or_else(|| "auto".into());
    (provider_id, provider, model)
}

/// Validate and execute a tool call through the central registry.
pub async fn execute_tool(
    name: &str,
    args: &Value,
    cwd: &str,
    timeout_secs: u64,
) -> ToolExecutionResult {
    let workspace = match crate::workspace::WorkspaceContext::discover(cwd) {
        Ok(workspace) => workspace,
        Err(error) => return ToolExecutionResult::Failure(error.to_string()),
    };
    execute_tool_in_workspace(name, args, &workspace, timeout_secs).await
}

pub async fn execute_tool_in_workspace(
    name: &str,
    args: &Value,
    workspace: &crate::workspace::WorkspaceContext,
    timeout_secs: u64,
) -> ToolExecutionResult {
    execute_tool_in_workspace_policy(
        name,
        args,
        workspace,
        timeout_secs,
        crate::sandbox::SandboxMode::Required,
        false,
    )
    .await
}

pub async fn execute_tool_in_workspace_policy(
    name: &str,
    args: &Value,
    workspace: &crate::workspace::WorkspaceContext,
    timeout_secs: u64,
    sandbox_mode: crate::sandbox::SandboxMode,
    network_allowed: bool,
) -> ToolExecutionResult {
    execute_tool_in_workspace_policy_owned(
        name,
        args,
        workspace,
        timeout_secs,
        sandbox_mode,
        network_allowed,
        crate::jobs::JobOwner::process(),
    )
    .await
}

#[allow(dead_code)] // ownerless compatibility entry point
pub async fn execute_prepared_tool_in_workspace_policy_with_skills(
    name: &str,
    prepared_args: &Value,
    workspace: &crate::workspace::WorkspaceContext,
    timeout_secs: u64,
    sandbox_mode: crate::sandbox::SandboxMode,
    network_allowed: bool,
    skills: &std::sync::Arc<std::sync::Mutex<crate::skills::SkillRegistry>>,
) -> ToolExecutionResult {
    execute_prepared_tool_in_workspace_policy_owned_with_skills(
        name,
        prepared_args,
        workspace,
        timeout_secs,
        sandbox_mode,
        network_allowed,
        crate::jobs::JobOwner::process(),
        skills,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn execute_tool_in_workspace_policy_owned(
    name: &str,
    args: &Value,
    workspace: &crate::workspace::WorkspaceContext,
    timeout_secs: u64,
    sandbox_mode: crate::sandbox::SandboxMode,
    network_allowed: bool,
    job_owner: crate::jobs::JobOwner,
) -> ToolExecutionResult {
    let prepared = match crate::workspace::prepare_tool_invocation(
        workspace,
        name,
        args,
        crate::agent::registry::ToolOrigin::BuiltIn,
    ) {
        Ok(prepared) => prepared,
        Err(error) => return ToolExecutionResult::Failure(error.to_string()),
    };
    execute_prepared_tool_in_workspace_policy_owned(
        name,
        &prepared.arguments,
        workspace,
        timeout_secs,
        sandbox_mode,
        network_allowed,
        job_owner,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn execute_prepared_tool_in_workspace_policy_owned(
    name: &str,
    prepared_args: &Value,
    workspace: &crate::workspace::WorkspaceContext,
    timeout_secs: u64,
    sandbox_mode: crate::sandbox::SandboxMode,
    network_allowed: bool,
    job_owner: crate::jobs::JobOwner,
) -> ToolExecutionResult {
    execute_prepared_tool_in_workspace_policy_owned_with_job_policy(
        name,
        prepared_args,
        workspace,
        timeout_secs,
        sandbox_mode,
        network_allowed,
        job_owner,
        crate::jobs::JobExecutionPolicy::default(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn execute_prepared_tool_in_workspace_policy_owned_with_job_policy(
    name: &str,
    prepared_args: &Value,
    workspace: &crate::workspace::WorkspaceContext,
    timeout_secs: u64,
    sandbox_mode: crate::sandbox::SandboxMode,
    network_allowed: bool,
    job_owner: crate::jobs::JobOwner,
    job_execution_policy: crate::jobs::JobExecutionPolicy,
) -> ToolExecutionResult {
    let context = ToolContext {
        recovery: crate::owned_tools::current_recovery(),
        cwd: workspace.cwd().to_path_buf(),
        timeout_secs,
        profile: ToolProfile::Main,
        workspace_roots: workspace.execution_roots(),
        sandbox_mode,
        network_allowed,
        job_owner,
        job_execution_policy,
    };
    execute_tool_with_context(name, prepared_args, &context).await
}

#[allow(clippy::too_many_arguments)]
pub async fn execute_prepared_tool_in_workspace_policy_owned_with_skills(
    name: &str,
    prepared_args: &Value,
    workspace: &crate::workspace::WorkspaceContext,
    timeout_secs: u64,
    sandbox_mode: crate::sandbox::SandboxMode,
    network_allowed: bool,
    job_owner: crate::jobs::JobOwner,
    skills: &std::sync::Arc<std::sync::Mutex<crate::skills::SkillRegistry>>,
) -> ToolExecutionResult {
    if name == "session_search" {
        let registry = ToolRegistry::builtins();
        if let Err(error) = registry.validate(name, prepared_args) {
            return ToolExecutionResult::Failure(error.to_string());
        }
        return match crate::session_recall::execute_process_json(
            prepared_args.clone(),
            workspace.cwd(),
        )
        .await
        {
            Ok(output) => ToolExecutionResult::Success(output),
            Err(error) => ToolExecutionResult::Failure(error),
        };
    }
    if name == "skill_manage" {
        let registry = ToolRegistry::builtins();
        if let Err(error) = registry.validate(name, prepared_args) {
            return ToolExecutionResult::Failure(error.to_string());
        }
        let request = match serde_json::from_value::<crate::skills::SkillManageRequest>(
            prepared_args.clone(),
        ) {
            Ok(request) => request,
            Err(error) => {
                return ToolExecutionResult::Failure(format!(
                    "Invalid skill_manage arguments: {error}"
                ))
            }
        };
        let mut skills = skills.lock().unwrap_or_else(|poison| poison.into_inner());
        return match skills.skill_manage(request) {
            Ok(receipt) => match serde_json::to_string_pretty(&receipt) {
                Ok(receipt) => ToolExecutionResult::Success(receipt),
                Err(error) => ToolExecutionResult::Failure(error.to_string()),
            },
            Err(error) => ToolExecutionResult::Failure(error.to_string()),
        };
    }
    if name == "skill_view" {
        let registry = ToolRegistry::builtins();
        if let Err(error) = registry.validate(name, prepared_args) {
            return ToolExecutionResult::Failure(error.to_string());
        }
        let Some(instance_id) = prepared_args.get("instance_id").and_then(Value::as_str) else {
            return ToolExecutionResult::Failure("Invalid skill_view arguments".into());
        };
        let Some(relative_path) = prepared_args.get("relative_path").and_then(Value::as_str) else {
            return ToolExecutionResult::Failure("Invalid skill_view arguments".into());
        };
        let skills = skills.lock().unwrap_or_else(|poison| poison.into_inner());
        return match skills.skill_view(instance_id, relative_path) {
            Ok(content) => ToolExecutionResult::Success(content),
            Err(error) => ToolExecutionResult::Failure(error.to_string()),
        };
    }
    if name == "activate_skill" {
        let registry = ToolRegistry::builtins();
        if let Err(error) = registry.validate(name, prepared_args) {
            return ToolExecutionResult::Failure(error.to_string());
        }
        let Some(id) = prepared_args.get("id").and_then(Value::as_str) else {
            return ToolExecutionResult::Failure("Invalid activate_skill arguments".into());
        };
        let mut skills = skills.lock().unwrap_or_else(|poison| poison.into_inner());
        return match skills.activate_exact(id) {
            Ok(()) => ToolExecutionResult::Success(format!("Activated skill: {id}")),
            Err(error) => ToolExecutionResult::Failure(error.to_string()),
        };
    }
    execute_prepared_tool_in_workspace_policy_owned(
        name,
        prepared_args,
        workspace,
        timeout_secs,
        sandbox_mode,
        network_allowed,
        job_owner,
    )
    .await
}

pub async fn execute_tool_with_context(
    name: &str,
    args: &Value,
    context: &ToolContext,
) -> ToolExecutionResult {
    let registry = ToolRegistry::builtins();
    let Some(spec) = registry.get(name) else {
        return ToolExecutionResult::Failure(format!("Unknown tool: {name}"));
    };
    if !spec.profiles.contains(&context.profile) {
        return ToolExecutionResult::Failure(format!(
            "Tool {name} is unavailable in the {:?} profile",
            context.profile
        ));
    }
    if let Err(error) = registry.validate(name, &crate::formatting::plain_arguments(args)) {
        return ToolExecutionResult::Failure(error.to_string());
    }
    if matches!(
        spec.handler,
        ToolHandler::RunCommand | ToolHandler::AwaitJob
    ) {
        return execute_job_tool(spec.handler, args, context).await;
    }
    if matches!(spec.handler, ToolHandler::ReadLints) {
        let lint_cwd = args["path"].as_str().map_or_else(
            || context.cwd.to_string_lossy().into_owned(),
            |path| resolve_path(path, &context.cwd.to_string_lossy()),
        );
        let lint_args = serde_json::json!({
            "command": "cargo check --message-format=short",
            "cwd": lint_cwd,
            "background": false,
            "network": false,
        });
        return execute_job_tool(ToolHandler::RunCommand, &lint_args, context).await;
    }
    if matches!(spec.handler, ToolHandler::ViewImage) {
        return execute_view_image(args, context).await;
    }
    if matches!(spec.handler, ToolHandler::Browser) {
        return crate::browser::BrowserManager::process()
            .execute(args, context)
            .await;
    }
    if matches!(spec.handler, ToolHandler::ProposeMemory) {
        let scope = match args["scope"].as_str().unwrap_or_default() {
            "user" => crate::memory::MemoryScope::User,
            "workspace" => crate::memory::MemoryScope::Workspace,
            _ => {
                return ToolExecutionResult::Failure(
                    "Invalid memory proposal scope; use user or workspace".into(),
                )
            }
        };
        let kind = match (scope, args["kind"].as_str().unwrap_or_default()) {
            (crate::memory::MemoryScope::User, "preference") => {
                crate::memory::MemoryKind::UserPreference
            }
            (crate::memory::MemoryScope::User, "workflow") => {
                crate::memory::MemoryKind::UserWorkflow
            }
            (crate::memory::MemoryScope::Workspace, "convention") => {
                crate::memory::MemoryKind::WorkspaceConvention
            }
            (crate::memory::MemoryScope::Workspace, "decision") => {
                crate::memory::MemoryKind::WorkspaceDecision
            }
            (_, "external_fact") => crate::memory::MemoryKind::ExternalFact,
            _ => {
                return ToolExecutionResult::Failure(
                    "Invalid memory proposal kind for the selected scope".into(),
                )
            }
        };
        return match crate::memory::MemoryProposal::new(
            scope,
            kind,
            args["text"].as_str().unwrap_or_default(),
        ) {
            Ok(proposal) => ToolExecutionResult::MemoryProposal {
                marker: "Memory proposal queued for explicit review in /memory".into(),
                proposal,
            },
            Err(error) => ToolExecutionResult::Failure(error.to_string()),
        };
    }
    if matches!(spec.handler, ToolHandler::InstallBinary) {
        return execute_install_binary(args, context).await;
    }
    if matches!(
        spec.handler,
        ToolHandler::WriteFile
            | ToolHandler::EditFile
            | ToolHandler::HashlineEdit
            | ToolHandler::ApplyPatch
            | ToolHandler::DeleteFile
    ) {
        return execute_file_mutation(spec.handler, args, context).await;
    }
    let cwd = context.cwd.to_string_lossy();
    let output = execute_builtin(spec.handler, args, &cwd, context).await;
    let output = classify_output(output);
    if let Some(limit) = context.job_execution_policy.retained_output_limit() {
        crate::automation_dispatch::bound_tool_result(output, limit)
    } else {
        output
    }
}

fn classify_output(output: String) -> ToolExecutionResult {
    const FAILURE_PREFIXES: &[&str] = &[
        "Error",
        "Invalid",
        "Unknown",
        "Patch failed",
        "Glob error",
        "git blame failed",
        "Subagent error",
        "No image data",
    ];
    if FAILURE_PREFIXES
        .iter()
        .any(|prefix| output.starts_with(prefix))
    {
        ToolExecutionResult::Failure(output)
    } else {
        ToolExecutionResult::Success(output)
    }
}

async fn read_undo_state(path: &std::path::Path) -> Result<crate::undo::FileState, String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || crate::undo::read_file_state(&path))
        .await
        .map_err(|_| "could not inspect the file for undo".to_string())?
}

#[derive(Debug)]
pub(crate) struct HashlineEditOutput {
    pub content: String,
    start_line: usize,
    end_line: usize,
}

#[derive(Clone, Copy)]
struct LineSpan {
    start: usize,
    content_end: usize,
}

fn line_spans(content: &str) -> Vec<LineSpan> {
    let bytes = content.as_bytes();
    let mut spans = Vec::new();
    let mut start = 0_usize;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        let content_end = if index > start && bytes[index - 1] == b'\r' {
            index - 1
        } else {
            index
        };
        spans.push(LineSpan { start, content_end });
        start = index + 1;
    }
    if start < bytes.len() {
        spans.push(LineSpan {
            start,
            content_end: bytes.len(),
        });
    }
    spans
}

fn line_anchor(line: &str) -> String {
    let digest = Sha256::digest(line.as_bytes());
    format!(
        "{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3]
    )
}

pub(crate) fn apply_hashline_edit(
    content: &str,
    start_anchor: &str,
    end_anchor: &str,
    replacement: &str,
) -> Result<HashlineEditOutput, String> {
    let valid_anchor =
        |anchor: &str| anchor.len() == 8 && anchor.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !valid_anchor(start_anchor) || !valid_anchor(end_anchor) {
        return Err("anchors must be exactly eight hexadecimal characters".into());
    }
    if replacement.len() > crate::undo::MAX_UNDO_FILE_BYTES {
        return Err("replacement exceeds the reversible file-size bound".into());
    }
    let start_anchor = start_anchor.to_ascii_lowercase();
    let end_anchor = end_anchor.to_ascii_lowercase();
    let spans = line_spans(content);
    let anchors = spans
        .iter()
        .map(|span| line_anchor(&content[span.start..span.content_end]))
        .collect::<Vec<_>>();
    let matching = |needle: &str| {
        anchors
            .iter()
            .enumerate()
            .filter_map(|(index, anchor)| (anchor == needle).then_some(index))
            .collect::<Vec<_>>()
    };
    let starts = matching(&start_anchor);
    let ends = matching(&end_anchor);
    if starts.len() != 1 || ends.len() != 1 {
        return Err(if starts.is_empty() || ends.is_empty() {
            "one or both anchors are stale or missing".into()
        } else {
            "one or both anchors are ambiguous because the content hash is duplicated".into()
        });
    }
    let start_index = starts[0];
    let end_index = ends[0];
    if start_index > end_index {
        return Err("anchor range is reversed or noncontiguous".into());
    }
    let start_byte = spans[start_index].start;
    let end_byte = spans[end_index].content_end;
    let new_size = content
        .len()
        .saturating_sub(end_byte.saturating_sub(start_byte))
        .saturating_add(replacement.len());
    if new_size > crate::undo::MAX_UNDO_FILE_BYTES {
        return Err("anchored edit would exceed the reversible file-size bound".into());
    }
    let mut edited = String::with_capacity(new_size);
    edited.push_str(&content[..start_byte]);
    edited.push_str(replacement);
    edited.push_str(&content[end_byte..]);
    Ok(HashlineEditOutput {
        content: edited,
        start_line: start_index + 1,
        end_line: end_index + 1,
    })
}

fn bounded_replacement_preview(replacement: &str) -> String {
    let lines = replacement.lines().collect::<Vec<_>>();
    let preview = lines
        .iter()
        .take(10)
        .map(|line| format!("+{}", line.chars().take(120).collect::<String>()))
        .collect::<Vec<_>>()
        .join("\n");
    if lines.len() > 10 {
        format!("{preview}\n… ({} more lines)", lines.len() - 10)
    } else {
        preview
    }
}

async fn execute_file_mutation(
    handler: ToolHandler,
    args: &Value,
    context: &ToolContext,
) -> ToolExecutionResult {
    let (prepared, marker) = if crate::formatting::has_ticket(args) {
        match crate::formatting::consume(
            match handler {
                ToolHandler::WriteFile => "write_file",
                ToolHandler::EditFile => "edit_file",
                ToolHandler::ApplyPatch => "apply_patch",
                ToolHandler::HashlineEdit => "hashline_edit",
                _ => "",
            },
            args,
        ) {
            Ok(edit) => edit,
            Err(error) => return ToolExecutionResult::Failure(error),
        }
    } else {
        let path = args["path"].as_str().unwrap_or("");
        let resolved = std::path::PathBuf::from(resolve_path(path, &context.cwd.to_string_lossy()));
        let before = match read_undo_state(&resolved).await {
            Ok(state) => state,
            Err(error) => {
                return ToolExecutionResult::Failure(format!(
                    "Error preparing {} for a reversible mutation: {error}",
                    resolved.display()
                ))
            }
        };
        let (action, after, marker) = match handler {
            ToolHandler::WriteFile => {
                let content = args["content"].as_str().unwrap_or("").to_string();
                let after = match crate::undo::FileState::utf8(content.clone()) {
                    Ok(state) => state,
                    Err(error) => return ToolExecutionResult::Failure(format!("Error: {error}")),
                };
                let verb = if matches!(before, crate::undo::FileState::Missing) {
                    "Created"
                } else {
                    "Modified"
                };
                let line_count = content.lines().count();
                let preview = content
                    .lines()
                    .take(10)
                    .map(|line| format!("+{line}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let more = if line_count > 10 {
                    format!("\n… ({} more lines)", line_count - 10)
                } else {
                    String::new()
                };
                (
                    "write_file",
                    after,
                    format!(
                        "@@ {} ({verb}, {} bytes) @@\n{preview}{more}",
                        resolved.display(),
                        content.len()
                    ),
                )
            }
            ToolHandler::EditFile => {
                let crate::undo::FileState::Utf8(content) = &before else {
                    return ToolExecutionResult::Failure(format!(
                        "Error reading {}: file does not exist",
                        resolved.display()
                    ));
                };
                let old_string = args["old_string"].as_str().unwrap_or("");
                let new_string = args["new_string"].as_str().unwrap_or("");
                let count = content.matches(old_string).count();
                if count == 0 {
                    return ToolExecutionResult::Failure(format!(
                        "Error: '{old_string}' not found in {}",
                        resolved.display()
                    ));
                }
                if count > 1 {
                    return ToolExecutionResult::Failure(format!(
                    "Error: search string found {count} times in {}. Use a more unique string to edit only the intended location.",
                    resolved.display()
                ));
                }
                let new_content = content.replacen(old_string, new_string, 1);
                let after = match crate::undo::FileState::utf8(new_content.clone()) {
                    Ok(state) => state,
                    Err(error) => return ToolExecutionResult::Failure(format!("Error: {error}")),
                };
                let old_display: String = old_string.chars().take(120).collect();
                let new_display: String = new_string.chars().take(120).collect();
                (
                    "edit_file",
                    after,
                    format!(
                        "@@ {} @@\n-{old_display}\n+{new_display}",
                        resolved.display()
                    ),
                )
            }
            ToolHandler::HashlineEdit => {
                let crate::undo::FileState::Utf8(content) = &before else {
                    return ToolExecutionResult::Failure(format!(
                        "Error reading {}: file does not exist",
                        resolved.display()
                    ));
                };
                let start_anchor = args["start_anchor"].as_str().unwrap_or_default();
                let end_anchor = args["end_anchor"].as_str().unwrap_or_default();
                let replacement = args["replacement"].as_str().unwrap_or_default();
                let anchored =
                    match apply_hashline_edit(content, start_anchor, end_anchor, replacement) {
                        Ok(anchored) => anchored,
                        Err(error) => {
                            return ToolExecutionResult::Failure(format!("Error: {error}"))
                        }
                    };
                let after = match crate::undo::FileState::utf8(anchored.content.clone()) {
                    Ok(state) => state,
                    Err(error) => return ToolExecutionResult::Failure(format!("Error: {error}")),
                };
                (
                    "hashline_edit",
                    after,
                    format!(
                        "@@ {} (anchored lines {}-{}) @@\n{}",
                        resolved.display(),
                        anchored.start_line,
                        anchored.end_line,
                        bounded_replacement_preview(replacement)
                    ),
                )
            }
            ToolHandler::ApplyPatch => {
                let crate::undo::FileState::Utf8(original) = &before else {
                    return ToolExecutionResult::Failure(format!(
                        "Error reading {}: file does not exist",
                        resolved.display()
                    ));
                };
                let patch = args["patch"].as_str().unwrap_or("");
                let patched = match apply_unified_patch(original, patch) {
                    Ok(patched) => patched,
                    Err(error) => {
                        return ToolExecutionResult::Failure(format!(
                            "Patch failed for {}: {error}",
                            resolved.display()
                        ))
                    }
                };
                let after = match crate::undo::FileState::utf8(patched.clone()) {
                    Ok(state) => state,
                    Err(error) => return ToolExecutionResult::Failure(format!("Error: {error}")),
                };
                (
                    "apply_patch",
                    after,
                    format!(
                        "@@ {} (patched, {} -> {} chars) @@\n{patch}",
                        resolved.display(),
                        original.len(),
                        patched.len()
                    ),
                )
            }
            ToolHandler::DeleteFile => {
                if matches!(before, crate::undo::FileState::Missing) {
                    return ToolExecutionResult::Failure(format!(
                        "Error deleting {}: file does not exist",
                        resolved.display()
                    ));
                }
                (
                    "delete_file",
                    crate::undo::FileState::Missing,
                    format!(
                        "@@ {} (deleted) @@\n-{}",
                        resolved.display(),
                        resolved.display()
                    ),
                )
            }
            _ => unreachable!("non-file handler routed to file mutation executor"),
        };
        let prepared =
            match crate::undo::PreparedFileMutation::single(resolved, action, before, after) {
                Ok(prepared) => prepared,
                Err(error) => return ToolExecutionResult::Failure(error),
            };
        (prepared, marker)
    };
    // Register before spawning: dropping the async waiter cannot hide a queued
    // blocking mutation from turn retirement and sealing.
    let recovery_admission = if let Some(recovery) = context.recovery.clone() {
        match recovery.admit_mutation() {
            Ok(admission) => Some((recovery, admission)),
            Err(error) => return ToolExecutionResult::Failure(error.to_string()),
        }
    } else {
        None
    };
    let roots = if context.workspace_roots.is_empty() {
        vec![context.cwd.clone()]
    } else {
        context.workspace_roots.clone()
    };
    let target_parents = prepared
        .receipt()
        .receipts()
        .iter()
        .filter_map(|receipt| receipt.path.parent().map(std::path::Path::to_path_buf))
        .collect::<Vec<_>>();
    // Workspace locks use a shared owner-bound namespace independent of the
    // recovery/profile scope. Bare runners acquire the same exclusion without
    // creating a recovery journal.
    let activity_store = crate::recovery::RecoveryStore::new(context.cwd.clone());
    let activity = match tokio::task::spawn_blocking(move || {
        let mut roots = roots
            .iter()
            .map(std::fs::canonicalize)
            .collect::<std::io::Result<Vec<_>>>()?;
        for parent in target_parents {
            let parent = std::fs::canonicalize(parent)?;
            if !roots.iter().any(|root| parent.starts_with(root)) {
                roots.push(parent);
            }
        }
        activity_store.try_workspace_activity(&roots)
    })
    .await
    {
        Ok(Ok(activity)) => activity,
        Ok(Err(error)) => {
            return ToolExecutionResult::Failure(format!(
                "File mutation was not started: workspace activity admission failed: {error}"
            ))
        }
        Err(_) => {
            return ToolExecutionResult::Failure(
                "File mutation was not started: workspace activity worker stopped".into(),
            )
        }
    };
    let outcome = tokio::task::spawn_blocking(move || {
        // spawn_blocking continues after its waiter is aborted. Keep exclusion
        // in this closure until the write and durable receipt finish.
        let _activity = activity;
        if let Some((recovery, admission)) = recovery_admission {
            recovery.apply_admitted(prepared, admission)
        } else {
            crate::recovery::RecordedMutationOutcome {
                result: prepared.apply(),
                warning: None,
            }
        }
    })
    .await;
    match outcome {
        Ok(outcome) => match outcome.result {
            Ok(mutation) => ToolExecutionResult::SuccessMutation {
                marker: outcome.warning.map_or(marker.clone(), |warning| {
                    format!("{marker}\n[Recovery warning: {warning}]")
                }),
                mutation,
            },
            Err(error) => ToolExecutionResult::Failure(error),
        },
        Err(_) => ToolExecutionResult::Failure(
            "File mutation worker failed; inspect the recovery journal before retrying.".into(),
        ),
    }
}

async fn execute_view_image(args: &Value, context: &ToolContext) -> ToolExecutionResult {
    let path = args["path"].as_str().unwrap_or("");
    let resolved = resolve_path(path, &context.cwd.to_string_lossy());
    let extension = std::path::Path::new(&resolved)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    let supported_extension = extension.as_deref().is_some_and(|extension| {
        matches!(
            extension,
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff"
        )
    });
    if !supported_extension {
        return ToolExecutionResult::Failure("Unsupported image type".into());
    }
    let metadata = match tokio::fs::metadata(&resolved).await {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return ToolExecutionResult::Failure("Image path is not a regular file".into()),
        Err(_) => return ToolExecutionResult::Failure("Could not read image file".into()),
    };
    if metadata.len() > crate::image_content::MAX_COMPRESSED_IMAGE_BYTES as u64 {
        return ToolExecutionResult::Failure("Image exceeds the compressed-size limit".into());
    }
    let bytes = match tokio::fs::read(&resolved).await {
        Ok(bytes) => bytes,
        Err(_) => return ToolExecutionResult::Failure("Could not read image file".into()),
    };
    if let Err(error) = crate::provider_filter::scan_outbound(&bytes) {
        return ToolExecutionResult::Failure(crate::provider_filter::withheld_message(&error));
    }
    let guessed = match image::guess_format(&bytes) {
        Ok(format) => format,
        Err(_) => return ToolExecutionResult::Failure("Image is malformed or unsupported".into()),
    };
    let extension_matches = match extension.as_deref() {
        Some("png") => guessed == image::ImageFormat::Png,
        Some("jpg" | "jpeg") => guessed == image::ImageFormat::Jpeg,
        Some("webp") => guessed == image::ImageFormat::WebP,
        Some("gif") => guessed == image::ImageFormat::Gif,
        Some("bmp") => guessed == image::ImageFormat::Bmp,
        Some("tif" | "tiff") => guessed == image::ImageFormat::Tiff,
        _ => false,
    };
    if !extension_matches {
        return ToolExecutionResult::Failure(
            "Image contents do not match the filename extension".into(),
        );
    }
    let normalized = tokio::task::spawn_blocking(move || {
        std::panic::catch_unwind(|| crate::image_content::normalize_encoded_image(bytes))
            .map_err(|_| "Image decoding failed".to_string())?
    })
    .await;
    match normalized {
        Ok(Ok(image)) => ToolExecutionResult::SuccessImage {
            marker: image.safe_marker(),
            image,
        },
        Ok(Err(error)) => ToolExecutionResult::Failure(error),
        Err(_) => ToolExecutionResult::Failure("Image decoding failed".into()),
    }
}

async fn execute_install_binary(args: &Value, context: &ToolContext) -> ToolExecutionResult {
    let source = match args["source"].as_str() {
        Some(source) if !source.trim().is_empty() => context.cwd.join(source),
        _ => {
            return ToolExecutionResult::Failure(
                "install_binary requires a non-empty 'source' path".into(),
            )
        }
    };
    let destination = args["destination"]
        .as_str()
        .filter(|path| !path.is_empty())
        .map(std::path::PathBuf::from);
    let backup = args["backup"].as_bool().unwrap_or(true);
    let verify = args["verify"].as_bool().unwrap_or(true);
    let installed = tokio::task::spawn_blocking(move || {
        let current_exe = std::env::current_exe()
            .map_err(|error| format!("Cannot determine current executable: {error}"))?;
        let destination = destination.unwrap_or_else(|| current_exe.clone());
        let self_update = destination == current_exe
            || std::fs::canonicalize(&destination)
                .ok()
                .zip(std::fs::canonicalize(&current_exe).ok())
                .is_some_and(|(destination, executable)| destination == executable);
        crate::binary_install::install(crate::binary_install::InstallRequest {
            source,
            destination,
            backup,
            verify,
            self_update,
        })
    })
    .await;
    match installed {
        Ok(Ok(receipt)) => ToolExecutionResult::Success(receipt),
        Ok(Err(error)) => ToolExecutionResult::Failure(error),
        Err(error) => {
            ToolExecutionResult::Failure(format!("Binary installation task failed: {error}"))
        }
    }
}

async fn execute_builtin(
    handler: ToolHandler,
    args: &Value,
    cwd: &str,
    context: &ToolContext,
) -> String {
    if matches!(handler, ToolHandler::Task) {
        return "Error: agent manager context is unavailable".into();
    }
    let result = match handler {
        ToolHandler::ReadFile => {
            let path = args["path"].as_str().unwrap_or("");
            let resolved = resolve_path(path, cwd);
            let start_line = args["start_line"].as_u64().map(|value| value as usize);
            let end_line = args["end_line"].as_u64().map(|value| value as usize);
            let include_line_hashes = args["include_line_hashes"].as_bool().unwrap_or(false);
            read_file_range(&resolved, path, start_line, end_line, include_line_hashes)
                .await
                .unwrap_or_else(|error| format!("Error reading {resolved}: {error}"))
        }
        ToolHandler::ReadPdf => {
            let path = args["path"].as_str().unwrap_or("");
            let resolved = resolve_path(path, cwd);
            let start_page = args["start_page"].as_u64().map(|value| value as usize);
            let end_page = args["end_page"].as_u64().map(|value| value as usize);
            read_pdf_pages(&resolved, path, start_page, end_page)
                .await
                .unwrap_or_else(|error| format!("Error reading PDF: {error}"))
        }
        ToolHandler::ViewImage => "Error: image tool bypassed typed execution".into(),
        ToolHandler::Browser => "Error: browser tool bypassed typed execution".into(),
        ToolHandler::WriteFile => "Error: file mutation bypassed typed execution".into(),
        ToolHandler::ListDir => {
            let path = args["path"].as_str().unwrap_or(".");
            let resolved = resolve_path(path, cwd);
            match tokio::fs::read_dir(&resolved).await {
                Ok(mut entries) => {
                    let mut items = Vec::new();
                    let deadline = std::time::Instant::now()
                        + Duration::from_secs(context.timeout_secs.clamp(1, 30));
                    let mut limited = false;
                    let mut bytes = 0;
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let name = entry.file_name().to_string_lossy().to_string();
                        bytes += name.len() + 2;
                        if items.len() >= 5000
                            || bytes > QUERY_MAX_OUTPUT_BYTES.saturating_sub(256)
                            || std::time::Instant::now() >= deadline
                        {
                            limited = true;
                            break;
                        }
                        let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
                        items.push(if is_dir { format!("{}/", name) } else { name });
                    }
                    items.sort();
                    if limited {
                        items.push(
                            "[Directory listing limited by entry, output or deadline budget]"
                                .into(),
                        );
                    }
                    items.join("\n")
                }
                Err(e) => format!("Error listing {}: {}", resolved, e),
            }
        }
        ToolHandler::RunCommand | ToolHandler::AwaitJob => {
            "Error: job tool bypassed typed execution".into()
        }
        ToolHandler::RunPrivileged => {
            "Error: run_privileged is available only through the host-owned interactive TUI handoff"
                .into()
        }
        ToolHandler::WriteShellStdin => {
            let Some(id) = parse_job_id(args) else {
                return "Invalid or missing job_id".into();
            };
            match crate::jobs::JobManager::process()
                .write_stdin_owned(id, args["input"].as_str().unwrap_or(""), context.job_owner)
                .await
            {
                Ok(()) => format!("Wrote stdin to job {id}"),
                Err(error) => error,
            }
        }
        ToolHandler::KillJob => {
            let Some(id) = parse_job_id(args) else {
                return "Invalid or missing job_id".into();
            };
            match crate::jobs::JobManager::process()
                .kill_owned(id, context.job_owner)
                .await
            {
                Ok(()) => format!("Cancellation requested for job {id}"),
                Err(error) => error,
            }
        }
        ToolHandler::WatchProcess => {
            let request: ca_watch::WatchRequestV1 = match serde_json::from_value(args.clone()) {
                Ok(request) => request,
                Err(error) => return format!("Invalid watch_process arguments: {error}"),
            };
            if request.host.is_remote() && !context.network_allowed {
                return "Error: remote process watching requires approved network access".into();
            }
            match crate::process_watch::ProcessWatchManager::process()
                .start_owned(request, context.job_owner)
                .await
            {
                Ok(id) => format!("Started process watcher {id}"),
                Err(error) => format!("Error: {error}"),
            }
        }
        ToolHandler::StopWatcher => {
            let Some(id) = args["watch_id"]
                .as_str()
                .and_then(|value| value.parse::<ca_watch::WatchId>().ok())
            else {
                return "Invalid or missing watch_id".into();
            };
            match crate::process_watch::ProcessWatchManager::process()
                .stop_owned(id, context.job_owner)
                .await
            {
                Ok(()) => format!("Stopped watcher {id}; target process unaffected"),
                Err(error) => format!("Error: {error}"),
            }
        }
        ToolHandler::SearchFiles => execute_bounded_file_query(args, context, false).await,
        ToolHandler::EditFile
        | ToolHandler::HashlineEdit
        | ToolHandler::ApplyPatch
        | ToolHandler::DeleteFile => "Error: file mutation bypassed typed execution".into(),
        ToolHandler::Glob => execute_bounded_file_query(args, context, true).await,
        ToolHandler::WebSearch => {
            let query = args["query"].as_str().unwrap_or("");
            let url = format!("https://html.duckduckgo.com/html/?q={}", urlencoding(query));
            match reqwest::get(&url).await {
                Ok(resp) => match resp.text().await {
                    Ok(html) => {
                        if let Err(error) = crate::provider_filter::scan_outbound(html.as_bytes()) {
                            return crate::provider_filter::withheld_message(&error);
                        }
                        let mut snippets = Vec::new();
                        for chunk in html.split("result__snippet") {
                            if let Some(start) = chunk.find('>') {
                                let rest = &chunk[start + 1..];
                                if let Some(end) = rest.find('<') {
                                    let text = rest[..end].trim().to_string();
                                    if text.len() > 5 {
                                        snippets.push(text);
                                    }
                                }
                            }
                        }
                        let mut urls = Vec::new();
                        for chunk in html.split("result__url") {
                            if let Some(start) = chunk.find('>') {
                                let rest = &chunk[start + 1..];
                                if let Some(end) = rest.find('<') {
                                    let text = rest[..end].trim().to_string();
                                    if !text.is_empty() {
                                        urls.push(text);
                                    }
                                }
                            }
                        }
                        let mut out = String::new();
                        for (i, u) in urls.iter().take(5).enumerate() {
                            let s = snippets.get(i).map(|s| s.as_str()).unwrap_or("");
                            out.push_str(&format!("{}. {}\n   {}\n\n", i + 1, u, s));
                        }
                        if out.is_empty() {
                            "No results found".into()
                        } else {
                            out
                        }
                    }
                    Err(e) => format!("Error reading response: {}", e),
                },
                Err(e) => format!("Error searching: {}", e),
            }
        }
        ToolHandler::WebFetch => {
            let url = args["url"].as_str().unwrap_or("");
            web_fetch_bounded(url, Duration::from_secs(context.timeout_secs))
                .await
                .unwrap_or_else(|error| format!("Error fetching: {error}"))
        }
        ToolHandler::ReadLints => unreachable!("read_lints is routed through the managed job path"),
        ToolHandler::ReadTodos => {
            let path = args["path"].as_str().unwrap_or(".");
            let resolved = resolve_path(path, cwd);
            let re = &*TODO_RE;
            let mut results = Vec::new();
            let mut count = 0;
            if let Ok(entries) = walk_dir(&resolved) {
                for entry in entries {
                    if count >= 50 {
                        break;
                    }
                    if let Ok(content) = std::fs::read_to_string(&entry) {
                        if let Err(error) =
                            crate::provider_filter::scan_outbound(content.as_bytes())
                        {
                            return crate::provider_filter::withheld_message(&error);
                        }
                        for (line_num, line) in content.lines().enumerate() {
                            if re.is_match(line) {
                                results.push(format!(
                                    "{}:{}: {}",
                                    entry,
                                    line_num + 1,
                                    line.trim()
                                ));
                                count += 1;
                                if count >= 50 {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            if results.is_empty() {
                "No TODO/FIXME/HACK comments found".into()
            } else {
                results.join("\n")
            }
        }
        ToolHandler::ReadScratch => {
            let category = match args["category"].as_str() {
                None => None,
                Some("notes" | "note") => Some(crate::scratch::ScratchCategory::Note),
                Some("todos" | "todo") => Some(crate::scratch::ScratchCategory::Todo),
                Some("plans" | "plan") => Some(crate::scratch::ScratchCategory::Plan),
                Some(_) => return "Error: scratch category must be notes, todos, or plans".into(),
            };
            crate::scratch::read_active(category).unwrap_or_else(|error| format!("Error: {error}"))
        }
        ToolHandler::CreatePlan => {
            let title = args["title"].as_str().unwrap_or("Plan");
            let steps = args["steps"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                .unwrap_or_default();
            let mut out = format!("## {}\n\n", title);
            for (i, step) in steps.iter().enumerate() {
                out.push_str(&format!("- [ ] {}. {}\n", i + 1, step));
            }
            out
        }
        ToolHandler::ProposePlan
        | ToolHandler::PlanWorkflow
        | ToolHandler::ProposeAutomation
        | ToolHandler::ListLocalAutomations
        | ToolHandler::ConfigureAgentGraph
        | ToolHandler::EditPlan
        | ToolHandler::UpdatePlanTask
        | ToolHandler::RecordPlanEvidence => "Error: work-state tools require orchestration".into(),
        ToolHandler::ProposeMemory => "Error: memory proposal bypassed typed execution".into(),
        ToolHandler::RequestUserInput | ToolHandler::AskQuestion => {
            "Error: interaction tools require orchestration".into()
        }
        ToolHandler::RequestGraphClarification => {
            "Error: request_graph_clarification requires the agent graph lane runner".into()
        }
        ToolHandler::UpdateTodos => {
            let path = args["path"].as_str().unwrap_or("");
            let action = args["action"].as_str().unwrap_or("");
            let resolved = resolve_path(path, cwd);
            match tokio::fs::read_to_string(&resolved).await {
                Ok(content) => {
                    if let Err(error) = crate::provider_filter::scan_outbound(content.as_bytes()) {
                        return crate::provider_filter::withheld_message(&error);
                    }
                    let mut lines: Vec<String> = content.lines().map(String::from).collect();
                    match action {
                        "complete" => {
                            let line_num = args["line_number"].as_u64().unwrap_or(0) as usize;
                            if line_num == 0 || line_num > lines.len() {
                                format!("Error: invalid line number {}", line_num)
                            } else {
                                let original = lines[line_num - 1].clone();
                                if TODO_RE.is_match(&original) {
                                    let replaced = TODO_RE.replace(&original, "DONE");
                                    lines[line_num - 1] = replaced.to_string();
                                    match tokio::fs::write(&resolved, lines.join("\n")).await {
                                        Ok(_) => format!(
                                            "Line {}: DONE (was: {})",
                                            line_num,
                                            original.trim()
                                        ),
                                        Err(e) => format!("Error writing {}: {}", resolved, e),
                                    }
                                } else {
                                    format!("Error: line {} has no TODO/FIXME marker", line_num)
                                }
                            }
                        }
                        "add" => {
                            let text = args["text"].as_str().unwrap_or("");
                            let line_num = args["line_number"].as_u64().unwrap_or(0) as usize;
                            let new_line = format!("// TODO: {}", text);
                            if line_num > 0 && line_num <= lines.len() {
                                lines.insert(line_num - 1, new_line.clone());
                            } else {
                                lines.push(new_line.clone());
                            }
                            match tokio::fs::write(&resolved, lines.join("\n")).await {
                                Ok(_) => format!("Added: {}", new_line),
                                Err(e) => format!("Error writing {}: {}", resolved, e),
                            }
                        }
                        _ => format!(
                            "Error: unknown action '{}'. Use 'complete' or 'add'",
                            action
                        ),
                    }
                }
                Err(e) => format!("Error reading {}: {}", resolved, e),
            }
        }
        ToolHandler::CreateDirectory => {
            let path = args["path"].as_str().unwrap_or("");
            let resolved = resolve_path(path, cwd);
            match tokio::fs::create_dir_all(&resolved).await {
                Ok(_) => format!("Created directory {}", resolved),
                Err(e) => format!("Error creating {}: {}", resolved, e),
            }
        }
        ToolHandler::SwitchMode => {
            let mode = args["mode"].as_str().unwrap_or("agent");
            format!("[SWITCH_MODE:{}]", mode)
        }
        ToolHandler::SendMessage => {
            let message = args["message"].as_str().unwrap_or("");
            format!("[SEND_MESSAGE] {}", message)
        }
        ToolHandler::Fetch => {
            let url = args["url"].as_str().unwrap_or("");
            let dest = args["dest"].as_str().unwrap_or("");
            let resolved = resolve_path(dest, cwd);
            match reqwest::get(url).await {
                Ok(resp) => match resp.bytes().await {
                    Ok(bytes) => match tokio::fs::write(&resolved, &bytes).await {
                        Ok(_) => format!("Downloaded {} bytes to {}", bytes.len(), resolved),
                        Err(e) => format!("Error writing {}: {}", resolved, e),
                    },
                    Err(e) => format!("Error reading response: {}", e),
                },
                Err(e) => format!("Error fetching {}: {}", url, e),
            }
        }
        ToolHandler::Blame => {
            let path = args["path"].as_str().unwrap_or("");
            let resolved = resolve_path(path, cwd);
            let mut cmd = tokio::process::Command::new("git");
            cmd.arg("blame");
            if let (Some(start), Some(end)) =
                (args["start_line"].as_u64(), args["end_line"].as_u64())
            {
                cmd.arg("-L").arg(format!("{},{}", start, end));
            }
            cmd.arg(&resolved);
            crate::run_environment::apply_tokio(&mut cmd);
            match cmd.output().await {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if stdout.is_empty() {
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        format!("git blame failed: {}", stderr.trim())
                    } else {
                        truncate_output(&stdout, 4096)
                    }
                }
                Err(e) => format!("Error running git blame: {}", e),
            }
        }
        ToolHandler::SwitchBranch => {
            let branch = args["branch"].as_str().unwrap_or("");
            let mut command = tokio::process::Command::new("git");
            command.args(["checkout", branch]).current_dir(cwd);
            crate::run_environment::apply_tokio(&mut command);
            match command.output().await {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let combined = format!("{}{}", stdout, stderr).trim().to_string();
                    if combined.is_empty() {
                        format!("Switched to branch {}", branch)
                    } else {
                        combined
                    }
                }
                Err(e) => format!("Error: {}", e),
            }
        }
        ToolHandler::GenerateImage => {
            let prompt = args["prompt"].as_str().unwrap_or("");
            let size = args["size"].as_str().unwrap_or("1024x1024");
            let filename = args["filename"].as_str();
            let image_url_arg = args["image_url"].as_str().filter(|value| !value.is_empty());
            let gen_url = crate::run_environment::var("IMAGE_GEN_URL")
                .unwrap_or_else(|| "http://127.0.0.1:11434/v1/images/generations".into());
            let gen_model =
                crate::run_environment::var("IMAGE_GEN_MODEL").unwrap_or_else(|| "dall-e-3".into());
            let gen_key = crate::run_environment::var("IMAGE_GEN_API_KEY").unwrap_or_default();
            let out_name = match filename {
                Some(f) if !f.is_empty() => f.to_string(),
                _ => format!("image-{}.png", chrono::Utc::now().timestamp()),
            };
            let images_dir = format!("{}/.ca-images", cwd);
            let _ = tokio::fs::create_dir_all(&images_dir).await;
            let out_path = format!("{}/{}", images_dir, out_name);

            let client = reqwest::Client::new();

            // If image_url provided, read and base64-encode it
            let image_b64 = if let Some(url) = image_url_arg {
                if !url.is_empty() {
                    let img_data = if url.starts_with("http://") || url.starts_with("https://") {
                        match image_download_bytes(&client, url, false).await {
                            Ok(bytes) => bytes,
                            Err(error) => return error,
                        }
                    } else {
                        let resolved = resolve_path(url, cwd);
                        match tokio::fs::read(&resolved).await {
                            Ok(data) => data,
                            Err(e) => {
                                return format!("Error reading reference image {}: {}", resolved, e)
                            }
                        }
                    };
                    if let Err(error) = crate::provider_filter::scan_outbound(&img_data) {
                        return crate::provider_filter::withheld_message(&error);
                    }
                    use base64::Engine;
                    Some(base64::engine::general_purpose::STANDARD.encode(&img_data))
                } else {
                    None
                }
            } else {
                None
            };

            let mut body = serde_json::json!({
                "model": gen_model, "prompt": prompt, "size": size, "n": 1,
            });
            if let Some(b64) = &image_b64 {
                body["image"] = serde_json::Value::String(format!("data:image/png;base64,{}", b64));
            }
            match image_provider_response(&client, &gen_url, &body, &gen_key, None).await {
                Ok(json) => {
                    let img = &json["data"][0];
                    let bytes: Vec<u8> = if let Some(url) = img["url"].as_str() {
                        match image_download_bytes(&client, url, true).await {
                            Ok(bytes) => bytes,
                            Err(error) => return error,
                        }
                    } else if let Some(b64) = img["b64_json"].as_str() {
                        use base64::Engine;
                        base64::engine::general_purpose::STANDARD
                            .decode(b64)
                            .unwrap_or_default()
                    } else {
                        return "No image data in response".into();
                    };
                    match tokio::fs::write(&out_path, &bytes).await {
                        Ok(_) => format!("Image saved to {}", out_path),
                        Err(e) => format!("Error saving image: {}", e),
                    }
                }
                Err(error) => error,
            }
        }
        ToolHandler::RequestWriteAccess => "Error: request_write_access is available only through the host-owned interactive folder approval".into(),
        ToolHandler::InstallBinary => "Error: install_binary bypassed typed execution".into(),
        ToolHandler::Task => {
            let name = args["subagent_name"].as_str().unwrap_or("");
            let prompt = args["prompt"].as_str().unwrap_or("");
            let agents = crate::agent::subagent::load_subagents(cwd);
            if let Some(def) = agents.iter().find(|a| a.name == name) {
                let all_tools = default_tool_defs();
                let filtered: Vec<_> = if def.tools.is_empty() {
                    all_tools
                } else {
                    all_tools
                        .into_iter()
                        .filter(|t| def.tools.contains(&t.function.name))
                        .collect()
                };
                // Resolve provider for subagent (inherits default)
                let config = crate::config::CaConfig::load().unwrap_or_default();
                let (provider_id, provider_cfg, model_id) =
                    resolve_subagent_route(&config, def.model.as_deref());
                let client = crate::agent::AgentClient::with_request_store(
                    provider_id,
                    &provider_cfg,
                    &model_id,
                    crate::request::RequestStore::default(),
                );
                let result = Box::pin(crate::agent::subagent::run_subagent(
                    &client,
                    &filtered,
                    &def.system_prompt,
                    prompt,
                    cwd,
                ))
                .await;
                format!("[SUBAGENT_RESULT:{}]\n{}", name, result)
            } else {
                let available: Vec<String> = agents
                    .iter()
                    .map(|a| format!("{}: {}", a.name, a.description))
                    .collect();
                format!(
                    "Error: unknown subagent '{}'. Available:\n{}",
                    name,
                    available.join("\n")
                )
            }
        }
        ToolHandler::SpawnAgent
        | ToolHandler::SpawnAgentBatch
        | ToolHandler::AwaitAgent
        | ToolHandler::AwaitAgentBatch
        | ToolHandler::SendAgentMessage
        | ToolHandler::CancelAgent
        | ToolHandler::SetAgentSpawning
        | ToolHandler::CancelAgentJob
        | ToolHandler::ListAgentRoutes
        | ToolHandler::ListAgents => "Error: agent manager context is unavailable".into(),
        ToolHandler::ActivateSkill | ToolHandler::SkillView | ToolHandler::SkillManage => {
            "Error: skill context is unavailable".into()
        }
        ToolHandler::SessionSearch => {
            match crate::session_recall::execute_process_json(args.clone(), &context.cwd).await {
                Ok(output) => output,
                Err(error) => format!("Error: {error}"),
            }
        }
        ToolHandler::DelegateMechanistic => {
            "Error: mechanistic delegation context is unavailable".into()
        }
        ToolHandler::SearchMcpTools => "Error: MCP discovery context is unavailable".into(),
        ToolHandler::SearchTools => {
            "Error: built-in tool discovery requires an owned runner".into()
        }
        ToolHandler::LspSymbols
        | ToolHandler::LspDefinition
        | ToolHandler::LspReferences
        | ToolHandler::LspHover
        | ToolHandler::LspImplementation
        | ToolHandler::LspCallHierarchy => "Error: LSP navigation context is unavailable".into(),
        ToolHandler::ReportReviewFindings => {
            "Error: review reporting requires orchestration".into()
        }
        ToolHandler::EmailAccounts
        | ToolHandler::EmailList
        | ToolHandler::EmailRead
        | ToolHandler::EmailDraft
        | ToolHandler::EmailSend
        | ToolHandler::EmailReply => "Error: email manager context is unavailable".into(),
        ToolHandler::Mcp => "Error: MCP execution context is unavailable".into(),
        ToolHandler::TransparentMechanistic => {
            "Error: transparent mechanistic execution context is unavailable".into()
        }
        ToolHandler::RemoteMechanistic => {
            "Error: remote mechanistic execution context is unavailable".into()
        }
    };
    let limit = match handler {
        ToolHandler::ReadFile => READ_FILE_MAX_RESULT_BYTES,
        ToolHandler::ReadPdf => PDF_MAX_RESULT_BYTES,
        ToolHandler::WebFetch => WEB_FETCH_MAX_RESULT_BYTES,
        ToolHandler::SessionSearch => crate::session_recall::MAX_RESULT_BYTES,
        _ => 4096,
    };
    truncate_output(&result, limit)
}

async fn execute_job_tool(
    handler: ToolHandler,
    args: &Value,
    context: &ToolContext,
) -> ToolExecutionResult {
    execute_job_tool_with_policy(handler, args, context).await
}

async fn execute_job_tool_with_policy(
    handler: ToolHandler,
    args: &Value,
    context: &ToolContext,
) -> ToolExecutionResult {
    let manager = crate::jobs::JobManager::process();
    match handler {
        ToolHandler::RunCommand => {
            if let Err(error) =
                crate::command_mutations::begin(context.job_owner, &context.workspace_roots).await
            {
                return ToolExecutionResult::Failure(error);
            }
            let command = args["command"].as_str().unwrap_or("");
            let dir = args["cwd"].as_str().map_or_else(
                || context.cwd.to_string_lossy().into_owned(),
                |path| resolve_path(path, &context.cwd.to_string_lossy()),
            );
            let background = args["background"].as_bool().unwrap_or(false);
            let effective_network_allowed = context.network_allowed;
            let execution_policy = context
                .job_execution_policy
                .for_authorized_command(command, effective_network_allowed);
            match manager
                .start_sandboxed_owned_with_policy_and_origin(
                    command,
                    dir,
                    background,
                    Duration::from_secs(context.timeout_secs),
                    context.workspace_roots.clone(),
                    effective_network_allowed,
                    context.sandbox_mode,
                    context.job_owner,
                    execution_policy,
                    crate::jobs::JobOrigin::ModelTool,
                )
                .await
            {
                Ok(id) if background => {
                    ToolExecutionResult::Success(format!("Started background job {id}"))
                }
                Ok(id) => match manager
                    .await_foreground_job_owned(id, context.job_owner)
                    .await
                {
                    Ok(crate::jobs::ForegroundJobOutcome::Finished(job)) => {
                        classify_job_result(job, effective_network_allowed)
                    }
                    Ok(crate::jobs::ForegroundJobOutcome::Promoted(job)) => {
                        ToolExecutionResult::Success(format!("Started background job {}", job.id))
                    }
                    Err(error) => ToolExecutionResult::Failure(error),
                },
                Err(error) => ToolExecutionResult::Failure(error),
            }
        }
        ToolHandler::AwaitJob => {
            let Some(id) = parse_job_id(args) else {
                return ToolExecutionResult::Failure("Invalid or missing job_id".into());
            };
            let stop = args["stop"].as_bool().unwrap_or(false);
            let timeout_secs = args["timeout_secs"].as_u64();
            let grace_secs = args["grace_secs"].as_u64();
            if stop && timeout_secs.is_some() {
                return ToolExecutionResult::Failure(
                    "await_job stop conflicts with timeout_secs".into(),
                );
            }
            if !stop && grace_secs.is_some() {
                return ToolExecutionResult::Failure(
                    "await_job grace_secs requires stop=true".into(),
                );
            }
            if grace_secs.is_some_and(|seconds| seconds > 300) {
                return ToolExecutionResult::Failure(
                    "await_job grace_secs must be between 0 and 300".into(),
                );
            }
            if let Err(error) = manager
                .reserve_completion_await_owned(id, context.job_owner)
                .await
            {
                return ToolExecutionResult::Failure(error);
            }
            let result = if stop {
                match manager
                    .stop_and_await_owned(
                        id,
                        context.job_owner,
                        Duration::from_secs(grace_secs.unwrap_or(5)),
                    )
                    .await
                {
                    Ok(_) => {
                        manager
                            .finish_completion_await_owned(id, context.job_owner, false)
                            .await
                    }
                    Err(error) => {
                        let _ = manager
                            .finish_completion_await_owned(id, context.job_owner, true)
                            .await;
                        Err(error)
                    }
                }
            } else {
                // Bound every wait so a multi-hour job cannot freeze the turn.
                // Omitted timeout uses a short default; explicit timeouts are capped.
                // On timeout the job snapshot (often still Running) is returned so
                // the model can report progress instead of blocking forever.
                let wait_secs = timeout_secs
                    .unwrap_or(DEFAULT_AWAIT_JOB_SECS)
                    .clamp(1, MAX_AWAIT_JOB_SECS);
                let wait = manager.await_job_owned(id, context.job_owner);
                match tokio::time::timeout(Duration::from_secs(wait_secs), wait).await {
                    Ok(result) => match result {
                        Ok(_) => {
                            manager
                                .finish_completion_await_owned(id, context.job_owner, false)
                                .await
                        }
                        Err(error) => Err(error),
                    },
                    Err(_) => {
                        manager
                            .finish_completion_await_owned(id, context.job_owner, true)
                            .await
                    }
                }
            };
            match result {
                Ok(job) => classify_job_result(job, false),
                Err(error) => ToolExecutionResult::Failure(error),
            }
        }
        _ => ToolExecutionResult::Failure("Invalid job tool handler".into()),
    }
}

fn classify_job_result(job: crate::jobs::JobInfo, network_requested: bool) -> ToolExecutionResult {
    let output = job.formatted_output();
    let success = matches!(
        job.status,
        crate::jobs::JobStatus::Completed { exit_code: 0 }
            | crate::jobs::JobStatus::Running
            | crate::jobs::JobStatus::Cancelling
    );
    match job.status {
        crate::jobs::JobStatus::Completed { .. }
        | crate::jobs::JobStatus::Failed { .. }
        | crate::jobs::JobStatus::TimedOut
        | crate::jobs::JobStatus::Cancelled
        | crate::jobs::JobStatus::Running
        | crate::jobs::JobStatus::Cancelling => ToolExecutionResult::Command {
            model_text: output,
            observation: crate::command_output::CommandObservation::from_job(
                &job,
                network_requested,
            ),
            success,
            mutation: None,
        },
    }
}

#[derive(Default)]
struct BoundedLineBytes {
    head: Vec<u8>,
    tail: Vec<u8>,
    total: usize,
    anchor_hasher: Sha256,
    anchor_pending: Option<u8>,
}

impl BoundedLineBytes {
    fn push(&mut self, bytes: &[u8]) {
        for byte in bytes {
            if let Some(previous) = self.anchor_pending.replace(*byte) {
                self.anchor_hasher.update([previous]);
            }
        }
        self.total = self.total.saturating_add(bytes.len());
        let head_remaining = READ_FILE_LINE_EDGE_BYTES.saturating_sub(self.head.len());
        self.head
            .extend_from_slice(&bytes[..bytes.len().min(head_remaining)]);
        self.tail.extend_from_slice(bytes);
        if self.tail.len() > READ_FILE_LINE_EDGE_BYTES {
            let remove = self.tail.len() - READ_FILE_LINE_EDGE_BYTES;
            self.tail.drain(..remove);
        }
    }

    fn anchor(&self) -> String {
        let mut hasher = self.anchor_hasher.clone();
        if let Some(last) = self.anchor_pending.filter(|byte| *byte != b'\r') {
            hasher.update([last]);
        }
        let digest = hasher.finalize();
        format!(
            "{:02x}{:02x}{:02x}{:02x}",
            digest[0], digest[1], digest[2], digest[3]
        )
    }

    fn render(&self, line_number: usize) -> String {
        if self.total <= READ_FILE_LINE_EDGE_BYTES {
            return String::from_utf8(self.head.clone())
                .expect("the stream was validated as UTF-8")
                .trim_end_matches('\r')
                .to_string();
        }
        if self.total <= READ_FILE_LINE_EDGE_BYTES * 2 {
            let suffix_len = self.total - self.head.len();
            let mut full = self.head.clone();
            full.extend_from_slice(&self.tail[self.tail.len() - suffix_len..]);
            return String::from_utf8(full)
                .expect("the stream was validated as UTF-8")
                .trim_end_matches('\r')
                .to_string();
        }

        let head_end = floor_utf8_boundary(&self.head, self.head.len());
        let tail_start = utf8_suffix_start(&self.tail);
        let head = std::str::from_utf8(&self.head[..head_end]).expect("validated UTF-8 head");
        let tail = std::str::from_utf8(&self.tail[tail_start..]).expect("validated UTF-8 tail");
        let omitted = self
            .total
            .saturating_sub(head_end)
            .saturating_sub(self.tail.len() - tail_start);
        format!(
            "{head}\n[… {omitted} bytes omitted from line {line_number} …]\n{}",
            tail.trim_end_matches('\r')
        )
    }
}

async fn read_file_range(
    resolved: &str,
    requested_path: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
    include_line_hashes: bool,
) -> Result<String, String> {
    let start = start_line.unwrap_or(1);
    if start == 0 {
        return Err("start_line must be at least 1".into());
    }
    let end = end_line.unwrap_or_else(|| start.saturating_add(READ_FILE_DEFAULT_LINES - 1));
    if end < start {
        return Err("end_line must be greater than or equal to start_line".into());
    }
    if end.saturating_sub(start).saturating_add(1) > READ_FILE_MAX_LINES {
        return Err(format!(
            "requested line span exceeds the {READ_FILE_MAX_LINES}-line maximum"
        ));
    }

    let mut file = tokio::fs::File::open(resolved)
        .await
        .map_err(|error| error.to_string())?;
    let mut outbound = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut pending_utf8 = Vec::new();
    let mut current_line = 1_usize;
    let mut current = BoundedLineBytes::default();
    let mut selected = String::new();
    let mut total_bytes = 0_usize;
    let mut total_lines = 0_usize;
    let mut bytes_since_newline = 0_usize;

    loop {
        let count = file
            .read(&mut buffer)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        let chunk = &buffer[..count];
        if let Err(error) = crate::provider_filter::scan_outbound_chunk(&mut outbound, chunk) {
            return Err(crate::provider_filter::withheld_message(&error));
        }
        validate_utf8_chunk(&mut pending_utf8, chunk)?;
        total_bytes = total_bytes.saturating_add(count);

        let mut segment_start = 0;
        for (index, byte) in chunk.iter().enumerate() {
            if *byte != b'\n' {
                continue;
            }
            let segment = &chunk[segment_start..index];
            if (start..=end).contains(&current_line) {
                current.push(segment);
                if include_line_hashes {
                    selected.push_str(&format!("{} ", current.anchor()));
                }
                selected.push_str(&current.render(current_line));
                selected.push('\n');
            }
            total_lines = current_line;
            current_line = current_line.saturating_add(1);
            current = BoundedLineBytes::default();
            bytes_since_newline = 0;
            segment_start = index + 1;
        }
        let remaining = &chunk[segment_start..];
        bytes_since_newline = bytes_since_newline.saturating_add(remaining.len());
        if (start..=end).contains(&current_line) {
            current.push(remaining);
        }
    }
    if let Err(error) = crate::provider_filter::scan_outbound_chunk(&mut outbound, &[]) {
        return Err(crate::provider_filter::withheld_message(&error));
    }
    if !pending_utf8.is_empty() {
        return Err("file ends with incomplete UTF-8".into());
    }
    if bytes_since_newline > 0 {
        if (start..=end).contains(&current_line) {
            if include_line_hashes {
                selected.push_str(&format!("{} ", current.anchor()));
            }
            selected.push_str(&current.render(current_line));
            selected.push('\n');
        }
        total_lines = current_line;
    }

    let shown_start = start.min(total_lines.saturating_add(1));
    let shown_end = end.min(total_lines);
    let showing = if shown_start <= shown_end {
        format!("Showing lines {shown_start}-{shown_end} of {total_lines}")
    } else {
        format!("Showing no lines (file has {total_lines})")
    };
    let before = start.saturating_sub(1).min(total_lines);
    let after = total_lines.saturating_sub(shown_end);
    let mut footer = format!(
        "\n{showing} · {total_bytes} bytes total\nOmitted lines: {before} before, {after} after"
    );
    if shown_end < total_lines {
        let next_start = shown_end.saturating_add(1).max(1);
        let next_end = next_start
            .saturating_add(READ_FILE_DEFAULT_LINES - 1)
            .min(total_lines);
        let mut next_range = serde_json::json!({
            "path": requested_path,
            "start_line": next_start,
            "end_line": next_end,
        });
        if include_line_hashes {
            next_range["include_line_hashes"] = Value::Bool(true);
        }
        footer.push_str(&format!("\nNext range: {next_range}"));
    }
    let body_budget = READ_FILE_MAX_RESULT_BYTES.saturating_sub(footer.len());
    let body = truncate_output(&selected, body_budget);
    Ok(format!("{body}{footer}"))
}

async fn read_pdf_pages(
    resolved: &str,
    requested_path: &str,
    start_page: Option<usize>,
    end_page: Option<usize>,
) -> Result<String, String> {
    let start = start_page.unwrap_or(1);
    if start == 0 {
        return Err("start_page must be at least 1".into());
    }
    let end = end_page.unwrap_or(start);
    if end < start {
        return Err("end_page must be greater than or equal to start_page".into());
    }
    if end.saturating_sub(start).saturating_add(1) > PDF_MAX_RANGE {
        return Err(format!(
            "requested page span exceeds the {PDF_MAX_RANGE}-page maximum"
        ));
    }
    let path = std::path::Path::new(resolved);
    if !path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
    {
        return Err("path must name a .pdf file".into());
    }
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|_| "PDF file is unavailable".to_string())?;
    if !metadata.is_file() {
        return Err("PDF path is not a regular file".into());
    }
    if metadata.len() > PDF_MAX_FILE_BYTES {
        return Err(format!(
            "PDF exceeds the {PDF_MAX_FILE_BYTES}-byte file limit"
        ));
    }
    let file_bytes = metadata.len();
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| "PDF file could not be read".to_string())?;
    let requested_path = requested_path.to_string();
    let parsed = tokio::task::spawn_blocking(move || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            extract_pdf_pages(bytes, &requested_path, file_bytes, start, end)
        }))
    })
    .await
    .map_err(|_| "PDF parsing worker failed".to_string())?;
    match parsed {
        Ok(result) => result,
        Err(_) => Err("PDF is malformed or unsupported".into()),
    }
}

fn extract_pdf_pages(
    bytes: Vec<u8>,
    requested_path: &str,
    file_bytes: u64,
    start: usize,
    end: usize,
) -> Result<String, String> {
    if let Err(error) = crate::provider_filter::scan_outbound(&bytes) {
        return Err(crate::provider_filter::withheld_message(&error));
    }
    let document = pdf_extract::Document::load_mem(&bytes)
        .map_err(|_| "PDF is malformed or unsupported".to_string())?;
    if document.is_encrypted() {
        return Err("encrypted PDFs are not supported".into());
    }
    let page_numbers = document.get_pages().keys().copied().collect::<Vec<_>>();
    let total_pages = page_numbers.len();
    if total_pages == 0 {
        return Err("PDF contains no readable pages".into());
    }
    if total_pages > PDF_MAX_PAGES {
        return Err(format!(
            "PDF exceeds the {PDF_MAX_PAGES}-page document limit"
        ));
    }
    if start > total_pages || end > total_pages {
        return Err(format!(
            "requested page range exceeds the {total_pages}-page document"
        ));
    }

    let mut writer = PdfTextWriter::new(PDF_TEXT_CAP_BYTES);
    let mut shown_end = start.saturating_sub(1);
    let mut truncated_page = None;
    for ordinal in start..=end {
        if std::io::Write::write_all(&mut writer, format!("--- Page {ordinal} ---\n").as_bytes())
            .is_err()
        {
            truncated_page = Some(ordinal);
            break;
        }
        let extraction = {
            let output: &mut dyn std::io::Write = &mut writer;
            let mut output = pdf_extract::PlainTextOutput::new(output);
            pdf_extract::output_doc_page(&document, &mut output, page_numbers[ordinal - 1])
        };
        shown_end = ordinal;
        if extraction.is_err() {
            if crate::provider_filter::is_tripped() {
                return Err(crate::provider_filter::withheld_message(
                    &crate::provider_filter::current().stopped_error(),
                ));
            }
            if writer.truncated {
                truncated_page = Some(ordinal);
                break;
            }
            return Err(format!("PDF page {ordinal} is malformed or unsupported"));
        }
        if std::io::Write::write_all(&mut writer, b"\n").is_err() {
            truncated_page = Some(ordinal);
            break;
        }
    }

    let text = String::from_utf8(writer.bytes)
        .map_err(|_| "PDF text extraction produced invalid UTF-8".to_string())?;
    let text = crate::terminal_text::sanitize_terminal_text(&text);
    let shown = if shown_end >= start {
        format!("Showing pages {start}-{shown_end} of {total_pages}")
    } else {
        format!("Showing no complete pages of {total_pages}")
    };
    let before = start.saturating_sub(1);
    let after = total_pages.saturating_sub(shown_end);
    let mut footer = format!(
        "\n{shown} · {file_bytes} bytes total\nOmitted pages: {before} before, {after} after"
    );
    if truncated_page.is_some() || writer.truncated {
        footer.push_str("\nOutput limit reached; remaining selected-page text was not extracted");
    }
    let next_start = truncated_page.unwrap_or_else(|| shown_end.saturating_add(1));
    if next_start <= total_pages {
        let next_end = next_start
            .saturating_add(PDF_MAX_RANGE - 1)
            .min(total_pages);
        footer.push_str(&format!(
            "\nNext range: {}",
            serde_json::json!({
                "path": requested_path,
                "start_page": next_start,
                "end_page": next_end,
            })
        ));
    }
    if text.len().saturating_add(footer.len()) > PDF_MAX_RESULT_BYTES {
        return Err("PDF result metadata exceeded its output limit".into());
    }
    Ok(format!("{text}{footer}"))
}

struct PdfTextWriter {
    bytes: Vec<u8>,
    cap: usize,
    truncated: bool,
    outbound: Vec<u8>,
}

impl PdfTextWriter {
    fn new(cap: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(cap),
            cap,
            truncated: false,
            outbound: Vec::new(),
        }
    }
}

impl std::io::Write for PdfTextWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if let Err(error) = crate::provider_filter::scan_outbound_chunk(&mut self.outbound, buffer)
        {
            self.bytes.clear();
            self.truncated = true;
            return Err(std::io::Error::other(
                crate::provider_filter::withheld_message(&error),
            ));
        }
        if self.truncated {
            return Err(std::io::Error::other("PDF output limit reached"));
        }
        let text = std::str::from_utf8(buffer).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "PDF extractor emitted invalid UTF-8",
            )
        })?;
        let remaining = self.cap.saturating_sub(self.bytes.len());
        let mut count = remaining.min(text.len());
        while count > 0 && !text.is_char_boundary(count) {
            count -= 1;
        }
        self.bytes.extend_from_slice(&buffer[..count]);
        if count < buffer.len() {
            self.truncated = true;
        }
        Ok(count)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn validate_utf8_chunk(pending: &mut Vec<u8>, chunk: &[u8]) -> Result<(), String> {
    let mut bytes = std::mem::take(pending);
    bytes.extend_from_slice(chunk);
    match std::str::from_utf8(&bytes) {
        Ok(_) => Ok(()),
        Err(error) if error.error_len().is_none() => {
            let valid = error.valid_up_to();
            pending.extend_from_slice(&bytes[valid..]);
            Ok(())
        }
        Err(_) => Err("file is not valid UTF-8".into()),
    }
}

fn floor_utf8_boundary(bytes: &[u8], mut index: usize) -> usize {
    index = index.min(bytes.len());
    while index > 0 && std::str::from_utf8(&bytes[..index]).is_err() {
        index -= 1;
    }
    index
}

fn utf8_suffix_start(bytes: &[u8]) -> usize {
    (0..=bytes.len().min(3))
        .find(|start| std::str::from_utf8(&bytes[*start..]).is_ok())
        .unwrap_or(bytes.len())
}

async fn web_fetch_bounded(url: &str, timeout: Duration) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|_| "could not initialize the HTTP client".to_string())?;
    let response = client.get(url).send().await.map_err(safe_fetch_error)?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or(value)
                .trim()
                .to_ascii_lowercase()
        });
    if content_type
        .as_deref()
        .is_some_and(|kind| !safe_text_content_type(kind))
    {
        return Err(format!(
            "unsupported content type {}",
            content_type.as_deref().unwrap_or("unknown")
        ));
    }

    let declared_oversize = response
        .content_length()
        .is_some_and(|length| length > WEB_FETCH_MAX_DECODED_BYTES as u64);
    let mut stream = response.bytes_stream();
    let mut outbound = Vec::new();
    let mut body = Vec::with_capacity(WEB_FETCH_MAX_DECODED_BYTES.min(64 * 1024));
    let mut unread = declared_oversize;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(safe_fetch_error)?;
        if let Err(error) = crate::provider_filter::scan_fetched_page_chunk(&mut outbound, &chunk) {
            return Err(crate::provider_filter::withheld_message(&error));
        }
        let remaining = WEB_FETCH_MAX_DECODED_BYTES.saturating_sub(body.len());
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            unread = true;
            break;
        }
        body.extend_from_slice(&chunk);
        if body.len() == WEB_FETCH_MAX_DECODED_BYTES {
            if stream
                .next()
                .await
                .transpose()
                .map_err(safe_fetch_error)?
                .is_some()
            {
                unread = true;
            }
            break;
        }
    }

    if !unread {
        if let Err(error) = crate::provider_filter::scan_fetched_page_chunk(&mut outbound, &[]) {
            return Err(crate::provider_filter::withheld_message(&error));
        }
    }

    let decoded = match std::str::from_utf8(&body) {
        Ok(value) => value,
        Err(error) if unread && error.error_len().is_none() => {
            std::str::from_utf8(&body[..error.valid_up_to()])
                .map_err(|_| "response is not valid UTF-8".to_string())?
        }
        Err(_) => return Err("response is not valid UTF-8".into()),
    };
    let text = if crate::provider_filter::keeps_html_source() {
        decoded.to_string()
    } else {
        strip_html(decoded)
    };
    let unread_marker = "\n[… remaining response was not read …]";
    let budget = if unread {
        WEB_FETCH_MAX_RESULT_BYTES.saturating_sub(unread_marker.len())
    } else {
        WEB_FETCH_MAX_RESULT_BYTES
    };
    let mut output = truncate_output(&text, budget);
    if unread {
        output.push_str(unread_marker);
    }
    Ok(output)
}

fn safe_text_content_type(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || matches!(
            content_type,
            "application/json"
                | "application/ld+json"
                | "application/xml"
                | "application/xhtml+xml"
                | "application/javascript"
                | "application/x-javascript"
        )
        || content_type.ends_with("+json")
        || content_type.ends_with("+xml")
}

fn safe_fetch_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "request timed out".into()
    } else if error.is_connect() {
        "could not connect".into()
    } else if error.is_decode() {
        "response decoding failed".into()
    } else {
        "request failed".into()
    }
}

fn parse_job_id(args: &Value) -> Option<crate::jobs::JobId> {
    args["job_id"].as_str()?.parse().ok()
}

/// Default wait when `await_job` omits `timeout_secs`. Short enough that the
/// turn stays responsive; the model can call again or use `watch_process`.
const DEFAULT_AWAIT_JOB_SECS: u64 = 30;
/// Hard cap on a single `await_job` wait. Long jobs must not freeze the UI.
const MAX_AWAIT_JOB_SECS: u64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PatchLineKind {
    Context,
    Remove,
    Add,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PatchLine {
    kind: PatchLineKind,
    text: String,
    no_newline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PatchHunk {
    old_start: usize,
    old_count: usize,
    new_count: usize,
    lines: Vec<PatchLine>,
}

const MAX_UNIFIED_PATCH_BYTES: usize = crate::undo::MAX_UNDO_FILE_BYTES;

impl PatchHunk {
    fn declared_index(&self) -> usize {
        if self.old_count == 0 {
            self.old_start
        } else {
            self.old_start.saturating_sub(1)
        }
    }

    fn old_image(&self) -> Vec<String> {
        self.lines
            .iter()
            .filter(|line| line.kind != PatchLineKind::Add)
            .map(|line| line.text.clone())
            .collect()
    }

    fn new_image(&self) -> Vec<String> {
        self.lines
            .iter()
            .filter(|line| line.kind != PatchLineKind::Remove)
            .map(|line| line.text.clone())
            .collect()
    }

    fn old_no_newline(&self) -> bool {
        self.lines
            .iter()
            .rev()
            .find(|line| line.kind != PatchLineKind::Add)
            .is_some_and(|line| line.no_newline)
    }

    fn new_no_newline(&self) -> bool {
        self.lines
            .iter()
            .rev()
            .find(|line| line.kind != PatchLineKind::Remove)
            .is_some_and(|line| line.no_newline)
    }
}

pub(crate) fn apply_unified_patch(original: &str, patch: &str) -> Result<String, String> {
    if patch.len() > MAX_UNIFIED_PATCH_BYTES {
        return Err(patch_failure("patch exceeds the bounded input limit"));
    }
    let hunks = parse_unified_patch(patch)?;
    let (mut result, newline, mut final_newline) = split_patch_source(original)?;
    let mut cumulative_delta = 0_isize;
    let mut previous_declared_end = 0_usize;
    let mut previous_applied_end = 0_usize;

    for (index, hunk) in hunks.iter().enumerate() {
        let declared = hunk.declared_index();
        if index > 0 && declared < previous_declared_end {
            return Err(patch_failure("patch hunks overlap or are out of order"));
        }
        previous_declared_end = declared.saturating_add(hunk.old_count);

        let old_image = hunk.old_image();
        let new_image = hunk.new_image();
        let expected = declared.checked_add_signed(cumulative_delta);
        let applied = locate_patch_hunk(
            &result,
            &old_image,
            expected,
            final_newline,
            hunk.old_no_newline(),
        )?;
        if index > 0 && applied < previous_applied_end {
            return Err(patch_failure(
                "relocated patch hunks overlap or are out of order",
            ));
        }

        let old_end = applied + old_image.len();
        let touches_old_eof = old_end == result.len();
        if hunk.old_no_newline() && !touches_old_eof {
            return Err(patch_failure(
                "old no-newline marker does not describe end of file",
            ));
        }
        if touches_old_eof && !old_image.is_empty() {
            let expected_final_newline = !hunk.old_no_newline();
            if final_newline != expected_final_newline {
                return Err(patch_failure(
                    "end-of-file newline state does not match patch",
                ));
            }
        }

        result.splice(applied..old_end, new_image.iter().cloned());
        let new_end = applied + new_image.len();
        let touches_new_eof = new_end == result.len();
        if hunk.new_no_newline() && !touches_new_eof {
            return Err(patch_failure(
                "new no-newline marker does not describe end of file",
            ));
        }
        if touches_new_eof && (touches_old_eof || old_image.is_empty()) {
            final_newline = !hunk.new_no_newline();
        }

        previous_applied_end = new_end;
        cumulative_delta = cumulative_delta
            .checked_add(new_image.len() as isize - old_image.len() as isize)
            .ok_or_else(|| patch_failure("patch line offset overflow"))?;
    }

    let mut output = result.join(newline);
    if final_newline {
        output.push_str(newline);
    }
    Ok(output)
}

fn parse_unified_patch(patch: &str) -> Result<Vec<PatchHunk>, String> {
    let mut hunks = Vec::new();
    let mut current: Option<PatchHunk> = None;

    for line in patch.lines() {
        if line.starts_with("@@") {
            if let Some(hunk) = current.take() {
                validate_patch_hunk(&hunk)?;
                hunks.push(hunk);
            }
            let (old_start, old_count, new_count) = parse_patch_hunk_header(line)?;
            current = Some(PatchHunk {
                old_start,
                old_count,
                new_count,
                lines: Vec::new(),
            });
            continue;
        }

        if let Some(hunk) = current.as_mut() {
            if line == "\\ No newline at end of file" {
                let Some(previous) = hunk.lines.last_mut() else {
                    return Err(patch_failure("newline marker has no preceding patch line"));
                };
                if previous.no_newline {
                    return Err(patch_failure("duplicate no-newline marker"));
                }
                previous.no_newline = true;
                continue;
            }
            let (kind, text) = match line.as_bytes().first().copied() {
                Some(b' ') => (PatchLineKind::Context, &line[1..]),
                Some(b'-') => (PatchLineKind::Remove, &line[1..]),
                Some(b'+') => (PatchLineKind::Add, &line[1..]),
                _ => return Err(patch_failure("hunk line is missing a unified-diff prefix")),
            };
            hunk.lines.push(PatchLine {
                kind,
                text: text.to_string(),
                no_newline: false,
            });
            continue;
        }

        let metadata_allowed = line.is_empty()
            || line.starts_with("diff --git ")
            || line.starts_with("index ")
            || line.starts_with("--- ")
            || line.starts_with("+++ ");
        let forbidden = line.starts_with("GIT binary patch")
            || line.starts_with("Binary files ")
            || line.starts_with("rename from ")
            || line.starts_with("rename to ")
            || line.starts_with("old mode ")
            || line.starts_with("new mode ")
            || line.starts_with("new file mode ")
            || line.starts_with("deleted file mode ")
            || ((line.starts_with("--- ") || line.starts_with("+++ "))
                && line.split_whitespace().nth(1) == Some("/dev/null"));
        if forbidden || !metadata_allowed {
            return Err(patch_failure(
                "unsupported file operation or patch metadata",
            ));
        }
    }

    if let Some(hunk) = current {
        validate_patch_hunk(&hunk)?;
        hunks.push(hunk);
    }
    if hunks.is_empty() {
        return Err(patch_failure("no valid unified-diff hunks found"));
    }
    Ok(hunks)
}

fn parse_patch_hunk_header(line: &str) -> Result<(usize, usize, usize), String> {
    let Some(end) = line[2..].find("@@").map(|offset| offset + 2) else {
        return Err(patch_failure("malformed hunk header"));
    };
    let ranges = line[2..end].split_whitespace().collect::<Vec<_>>();
    if ranges.len() != 2 || !ranges[0].starts_with('-') || !ranges[1].starts_with('+') {
        return Err(patch_failure("malformed hunk header"));
    }
    let (old_start, old_count) = parse_patch_range(&ranges[0][1..])?;
    let (_new_start, new_count) = parse_patch_range(&ranges[1][1..])?;
    Ok((old_start, old_count, new_count))
}

fn parse_patch_range(value: &str) -> Result<(usize, usize), String> {
    let mut parts = value.split(',');
    let start = parts
        .next()
        .and_then(|part| part.parse::<usize>().ok())
        .ok_or_else(|| patch_failure("malformed hunk range"))?;
    let count = parts
        .next()
        .map(|part| part.parse::<usize>())
        .transpose()
        .map_err(|_| patch_failure("malformed hunk range"))?
        .unwrap_or(1);
    if parts.next().is_some() || (start == 0 && count != 0) {
        return Err(patch_failure("malformed hunk range"));
    }
    Ok((start, count))
}

fn validate_patch_hunk(hunk: &PatchHunk) -> Result<(), String> {
    let old_count = hunk
        .lines
        .iter()
        .filter(|line| line.kind != PatchLineKind::Add)
        .count();
    let new_count = hunk
        .lines
        .iter()
        .filter(|line| line.kind != PatchLineKind::Remove)
        .count();
    if old_count != hunk.old_count || new_count != hunk.new_count {
        return Err(patch_failure("hunk line count does not match its header"));
    }
    for (index, line) in hunk.lines.iter().enumerate() {
        if !line.no_newline {
            continue;
        }
        let later_old = hunk.lines[index + 1..]
            .iter()
            .any(|later| later.kind != PatchLineKind::Add);
        let later_new = hunk.lines[index + 1..]
            .iter()
            .any(|later| later.kind != PatchLineKind::Remove);
        if (line.kind != PatchLineKind::Add && later_old)
            || (line.kind != PatchLineKind::Remove && later_new)
        {
            return Err(patch_failure(
                "no-newline marker is not at end of hunk image",
            ));
        }
    }
    Ok(())
}

fn split_patch_source(original: &str) -> Result<(Vec<String>, &'static str, bool), String> {
    let newline = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let final_newline = original.ends_with('\n');
    let mut lines = Vec::new();
    for chunk in original.split_inclusive('\n') {
        let mut line = chunk.strip_suffix('\n').unwrap_or(chunk);
        if newline == "\r\n" && chunk.ends_with('\n') {
            line = line
                .strip_suffix('\r')
                .ok_or_else(|| patch_failure("source has mixed newline styles"))?;
        } else if newline == "\n" && line.ends_with('\r') {
            return Err(patch_failure("source has mixed newline styles"));
        }
        lines.push(line.to_string());
    }
    if original.is_empty() {
        lines.clear();
    }
    Ok((lines, newline, final_newline))
}

fn locate_patch_hunk(
    lines: &[String],
    old_image: &[String],
    expected: Option<usize>,
    final_newline: bool,
    old_no_newline: bool,
) -> Result<usize, String> {
    let matches_at = |index: usize| {
        index <= lines.len()
            && index + old_image.len() <= lines.len()
            && lines[index..index + old_image.len()] == *old_image
            && (!old_no_newline || (index + old_image.len() == lines.len() && !final_newline))
    };
    if let Some(expected) = expected.filter(|index| matches_at(*index)) {
        return Ok(expected);
    }
    if old_image.is_empty() {
        return expected
            .filter(|index| *index <= lines.len())
            .ok_or_else(|| patch_failure("insertion offset is outside the file"));
    }
    let candidates = (0..=lines.len().saturating_sub(old_image.len()))
        .filter(|index| matches_at(*index))
        .take(2)
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [only] => Ok(*only),
        [] => Err(patch_failure("hunk context or deletion does not match")),
        _ => Err(patch_failure("hunk relocation is ambiguous")),
    }
}

fn patch_failure(reason: &str) -> String {
    format!("{reason}; file unchanged; re-read the file and regenerate the patch")
}

pub fn resolve_path(path: &str, cwd: &str) -> String {
    if let Some(rest) = path.strip_prefix('~') {
        std::env::var_os("HOME")
            .map(|home| format!("{}{}", std::path::PathBuf::from(home).display(), rest))
            .unwrap_or_else(|| path.to_string())
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{}/{}", cwd, path)
    }
}

const QUERY_MAX_ENTRIES: usize = 50_000;
const QUERY_MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
const QUERY_MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;
const QUERY_MAX_RESULTS: usize = 100;
const QUERY_MAX_OUTPUT_BYTES: usize = 64 * 1024;

struct QueryCancellation(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Drop for QueryCancellation {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
}

enum FileQuery {
    Search(regex::Regex),
    Glob(glob::Pattern),
}

async fn execute_bounded_file_query(
    args: &Value,
    context: &ToolContext,
    glob_query: bool,
) -> String {
    let pattern = args["pattern"].as_str().unwrap_or("");
    if pattern.len() > 16 * 1024 {
        return "Error: query pattern exceeds 16KiB".into();
    }
    let query = if glob_query {
        if std::path::Path::new(pattern).is_absolute()
            || std::path::Path::new(pattern)
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return "Glob error: pattern must remain within the prepared base directory".into();
        }
        match glob::Pattern::new(pattern) {
            Ok(pattern) => FileQuery::Glob(pattern),
            Err(error) => return format!("Glob error: {error}"),
        }
    } else {
        match regex::RegexBuilder::new(pattern)
            .size_limit(1024 * 1024)
            .build()
        {
            Ok(pattern) => FileQuery::Search(pattern),
            Err(error) => return format!("Invalid regex: {error}"),
        }
    };
    let base = std::path::PathBuf::from(resolve_path(
        args["path"].as_str().unwrap_or("."),
        &context.cwd.to_string_lossy(),
    ));
    let roots = context.workspace_roots.clone();
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let _cancel_on_drop = QueryCancellation(cancelled.clone());
    let deadline =
        std::time::Instant::now() + Duration::from_secs(context.timeout_secs.clamp(1, 30));
    static QUERY_WORKERS: LazyLock<std::sync::Arc<tokio::sync::Semaphore>> = LazyLock::new(|| {
        std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_NATIVE_READ_CONCURRENCY))
    });
    let permit = match tokio::time::timeout(
        Duration::from_secs(context.timeout_secs.clamp(1, 30)),
        QUERY_WORKERS.clone().acquire_owned(),
    )
    .await
    {
        Ok(Ok(permit)) => permit,
        _ => return "Error: file query worker admission timed out".into(),
    };
    match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        bounded_file_query(base, roots, query, cancelled, deadline)
    })
    .await
    {
        Ok(result) => result,
        Err(_) => "Error: file query worker failed".into(),
    }
}

fn bounded_file_query(
    base: std::path::PathBuf,
    roots: Vec<std::path::PathBuf>,
    query: FileQuery,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    deadline: std::time::Instant,
) -> String {
    use std::io::Read;
    let base = match std::fs::canonicalize(&base) {
        Ok(base) => base,
        Err(error) => return format!("Error: query base unavailable: {error}"),
    };
    let roots = roots
        .iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .collect::<Vec<_>>();
    let contained = |path: &std::path::Path| roots.iter().any(|root| path.starts_with(root));
    if !contained(&base) {
        return "Error: query base is outside prepared workspace roots".into();
    }
    let mut stack = vec![base.clone()];
    let mut output = String::new();
    let mut matches = 0;
    let mut visited = 0;
    let mut total_bytes = 0;
    let mut skipped = 0;
    let mut limited = None;
    while let Some(path) = stack.pop() {
        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
            return "Error: file query cancelled".into();
        }
        if std::time::Instant::now() >= deadline {
            limited = Some("deadline");
            break;
        }
        visited += 1;
        if visited > QUERY_MAX_ENTRIES {
            limited = Some("entry budget");
            break;
        }
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        // Never recurse through directory symlinks or open symlink/non-regular candidates.
        if metadata.file_type().is_symlink() {
            skipped += 1;
            continue;
        }
        let canonical = match std::fs::canonicalize(&path) {
            Ok(path) if contained(&path) => path,
            _ => {
                skipped += 1;
                continue;
            }
        };
        if metadata.is_dir() {
            let entries = match std::fs::read_dir(&canonical) {
                Ok(entries) => entries,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let remaining = QUERY_MAX_ENTRIES.saturating_sub(visited + stack.len());
            let mut children = Vec::new();
            let mut scanned = 0;
            for entry in entries {
                if cancelled.load(std::sync::atomic::Ordering::Acquire)
                    || std::time::Instant::now() >= deadline
                {
                    limited = Some("deadline or cancellation");
                    break;
                }
                scanned += 1;
                if scanned > remaining {
                    limited = Some("entry budget");
                    break;
                }
                match entry {
                    Ok(entry)
                        if !is_hidden_or_skippable_file(&entry.path())
                            && !query_skips_directory_name(&entry.file_name()) =>
                    {
                        children.push(entry.path())
                    }
                    Ok(_) => {}
                    Err(_) => skipped += 1,
                }
            }
            children.sort();
            stack.extend(children.into_iter().rev());
            if limited.is_some() {
                break;
            }
            continue;
        }
        if !metadata.is_file() || is_hidden_or_skippable_file(&canonical) {
            skipped += 1;
            continue;
        }
        let mut append = |line: String| -> Option<usize> {
            if output.len() + line.len() + 1 > QUERY_MAX_OUTPUT_BYTES.saturating_sub(256) {
                return None;
            }
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&line);
            matches += 1;
            Some(matches)
        };
        match &query {
            FileQuery::Glob(pattern) => {
                let relative = canonical.strip_prefix(&base).unwrap_or(&canonical);
                if pattern.matches_path_with(
                    relative,
                    glob::MatchOptions {
                        case_sensitive: true,
                        require_literal_separator: true,
                        require_literal_leading_dot: true,
                    },
                ) && append(canonical.to_string_lossy().into_owned()).is_none()
                {
                    limited = Some("output budget");
                    break;
                }
            }
            FileQuery::Search(pattern) => {
                if metadata.len() > QUERY_MAX_FILE_BYTES as u64 {
                    skipped += 1;
                    continue;
                }
                if total_bytes + metadata.len() as usize > QUERY_MAX_TOTAL_BYTES {
                    limited = Some("byte budget");
                    break;
                }
                let mut options = std::fs::OpenOptions::new();
                options.read(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
                }
                let mut file = match options.open(&canonical) {
                    Ok(file) if file.metadata().is_ok_and(|metadata| metadata.is_file()) => file,
                    _ => {
                        skipped += 1;
                        continue;
                    }
                };
                let mut bytes = Vec::new();
                let mut outbound = Vec::new();
                let mut buffer = [0u8; 8192];
                loop {
                    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                        return "Error: file query cancelled".into();
                    }
                    if std::time::Instant::now() >= deadline {
                        limited = Some("deadline");
                        break;
                    }
                    let count = match file.read(&mut buffer) {
                        Ok(0) => {
                            if let Err(error) =
                                crate::provider_filter::scan_outbound_chunk(&mut outbound, &[])
                            {
                                return crate::provider_filter::withheld_message(&error);
                            }
                            break;
                        }
                        Ok(count) => count,
                        Err(_) => {
                            skipped += 1;
                            break;
                        }
                    };
                    total_bytes += count;
                    if let Err(error) =
                        crate::provider_filter::scan_outbound_chunk(&mut outbound, &buffer[..count])
                    {
                        return crate::provider_filter::withheld_message(&error);
                    }
                    if bytes.len() + count > QUERY_MAX_FILE_BYTES
                        || total_bytes > QUERY_MAX_TOTAL_BYTES
                    {
                        limited = Some("byte budget");
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                }
                if limited.is_some() {
                    break;
                }
                let content = match std::str::from_utf8(&bytes) {
                    Ok(content) => content,
                    Err(_) => {
                        skipped += 1;
                        continue;
                    }
                };
                for (index, line) in content.lines().enumerate() {
                    if pattern.is_match(line) {
                        let appended = append(format!(
                            "{}: {}: {}",
                            canonical.display(),
                            index + 1,
                            line.trim()
                        ));
                        if appended.is_none() {
                            limited = Some("output budget");
                            break;
                        }
                        if appended.is_some_and(|count| count >= QUERY_MAX_RESULTS) {
                            limited = Some("result budget");
                            break;
                        }
                    }
                }
            }
        }
        if limited.is_some() {
            break;
        }
        if matches >= QUERY_MAX_RESULTS {
            limited = Some("result budget");
            break;
        }
    }
    if output.is_empty() {
        output = if matches!(&query, FileQuery::Glob(_)) {
            "No files matched"
        } else {
            "No matches found"
        }
        .into();
    }
    if limited.is_some() || skipped > 0 {
        output.push_str(&format!(
            "\n[Query limited: {}; skipped {skipped} unreadable, excluded or oversized candidates]",
            limited.unwrap_or("candidate exclusions")
        ));
    }
    output
}

fn query_skips_directory_name(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str(),
        Some("target" | "node_modules" | "dist" | "build" | "__pycache__" | "venv")
    )
}

fn walk_dir(path: &str) -> Result<Vec<String>, std::io::Error> {
    let skip_dirs: &[&str] = &[
        ".git",
        "target",
        "node_modules",
        "dist",
        "build",
        "__pycache__",
        ".venv",
        "venv",
        ".cache",
        ".next",
        ".nuxt",
    ];
    let mut results = Vec::new();
    let path = std::path::Path::new(path);
    if path.is_file() {
        let name = path.to_string_lossy().to_string();
        if !is_hidden_or_skippable_file(path) {
            results.push(name);
        }
    } else if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let p = entry.path();
            let fname = p.file_name().unwrap_or_default().to_string_lossy();
            if p.is_file() {
                if !is_hidden_or_skippable_file(&p) {
                    results.push(p.to_string_lossy().to_string());
                }
            } else if p.is_dir() {
                if fname.starts_with('.') || skip_dirs.contains(&fname.as_ref()) {
                    continue;
                }
                results.extend(walk_dir(&p.to_string_lossy()).unwrap_or_default());
            }
        }
    }
    Ok(results)
}

fn is_hidden_or_skippable_file(p: &std::path::Path) -> bool {
    let fname = p.file_name().unwrap_or_default().to_string_lossy();
    fname.starts_with('.')
        || fname.ends_with(".exe")
        || fname.ends_with(".bin")
        || fname.ends_with(".o")
        || fname.ends_with(".so")
        || fname.ends_with(".dylib")
        || fname.ends_with(".dll")
}

fn truncate_output(s: &str, max: usize) -> String {
    if let Err(error) = crate::provider_filter::scan_outbound(s.as_bytes()) {
        return crate::provider_filter::withheld_message(&error);
    }
    if s.len() <= max {
        return s.to_string();
    }
    let mut retained = max.saturating_sub(32);
    loop {
        let head_target = retained.div_ceil(2);
        let tail_target = retained / 2;
        let mut head_end = head_target.min(s.len());
        while head_end > 0 && !s.is_char_boundary(head_end) {
            head_end -= 1;
        }
        let mut tail_start = s.len().saturating_sub(tail_target);
        while tail_start < s.len() && !s.is_char_boundary(tail_start) {
            tail_start += 1;
        }
        if tail_start < head_end {
            tail_start = head_end;
        }
        let omitted = tail_start.saturating_sub(head_end);
        let marker = format!("\n[… {omitted} bytes omitted …]\n");
        let output_len = head_end + marker.len() + s.len().saturating_sub(tail_start);
        if output_len <= max || retained == 0 {
            return format!("{}{}{}", &s[..head_end], marker, &s[tail_start..]);
        }
        retained = retained.saturating_sub(output_len - max);
    }
}

fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' => "+".to_string(),
            c if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') => c.to_string(),
            c => format!("%{:02X}", c as u8),
        })
        .collect()
}

fn strip_html(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        if c == '<' {
            in_tag = true;
            continue;
        }
        if c == '>' {
            in_tag = false;
            continue;
        }
        if !in_tag {
            result.push(c);
        }
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

// Asset fetches use the real URL, but a process trip must cancel their I/O too.
async fn image_download_bytes(
    client: &reqwest::Client,
    url: &str,
    provider_response: bool,
) -> Result<Vec<u8>, String> {
    let download = async { client.get(url).send().await?.bytes().await };
    let bytes = image_filter_guarded(&crate::provider_filter::current, download)
        .await?
        .map_err(|error| format!("Error downloading image: {error}"))?;
    if provider_response {
        let filter = crate::provider_filter::current();
        let mut carry = Vec::new();
        filter
            .observe_response(&mut carry, &bytes)
            .and_then(|()| filter.observe_response(&mut carry, &[]))
            .map_err(|error| crate::provider_filter::withheld_message(&error))?;
    }
    Ok(bytes.to_vec())
}

// Image generation has its own HTTP endpoint, but uses the same process gate.
async fn image_provider_response(
    client: &reqwest::Client,
    url: &str,
    body: &Value,
    api_key: &str,
    filter_override: Option<std::sync::Arc<crate::provider_filter::ProviderFilter>>,
) -> Result<Value, String> {
    let active_filter = || {
        filter_override
            .clone()
            .unwrap_or_else(crate::provider_filter::current)
    };
    let filter = active_filter();
    let encoded = serde_json::to_vec(body)
        .map_err(|error| format!("Error preparing image request: {error}"))?;
    let encoded = filter
        .prepare_request(&encoded)
        .map_err(|error| crate::provider_filter::withheld_message(&error))?;
    if filter.shows_banner() {
        crate::provider_filter::publish_outbound_body(&encoded);
    }
    let mut request = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(encoded);
    if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
    }
    let response = image_filter_guarded(&active_filter, request.send())
        .await?
        .map_err(|error| format!("Error contacting image API: {error}"))?;
    let status = response.status();
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    let mut carry = Vec::new();
    while let Some(chunk) = image_filter_guarded(&active_filter, stream.next()).await? {
        let chunk = chunk.map_err(|error| format!("Error reading image response: {error}"))?;
        active_filter()
            .observe_response(&mut carry, &chunk)
            .map_err(|error| crate::provider_filter::withheld_message(&error))?;
        bytes.extend_from_slice(&chunk);
    }
    active_filter()
        .observe_response(&mut carry, &[])
        .map_err(|error| crate::provider_filter::withheld_message(&error))?;
    // JSON escapes are transport syntax; stop rules also see decoded strings.
    let parsed = serde_json::from_slice::<Value>(&bytes);
    if let Ok(value) = &parsed {
        scan_image_response_strings(&active_filter(), value)?;
    }
    if !status.is_success() {
        let preview: String = String::from_utf8_lossy(&bytes).chars().take(200).collect();
        return Err(format!("Image API error {status}: {preview}"));
    }
    parsed.map_err(|error| format!("Error parsing response: {error}"))
}

async fn image_filter_guarded<F: std::future::Future>(
    active_filter: &impl Fn() -> std::sync::Arc<crate::provider_filter::ProviderFilter>,
    operation: F,
) -> Result<F::Output, String> {
    tokio::pin!(operation);
    loop {
        let filter = active_filter();
        tokio::select! {
            biased;
            _ = filter.until_tripped() => {
                return Err(crate::provider_filter::withheld_message(&filter.stopped_error()));
            }
            result = &mut operation => return Ok(result),
            // /filter on replaces the process snapshot while this I/O waits.
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    }
}

fn scan_image_response_strings(
    filter: &crate::provider_filter::ProviderFilter,
    value: &Value,
) -> Result<(), String> {
    match value {
        Value::String(text) => {
            let mut carry = Vec::new();
            filter
                .observe_response(&mut carry, text.as_bytes())
                .and_then(|()| filter.observe_response(&mut carry, &[]))
                .map_err(|error| crate::provider_filter::withheld_message(&error))
        }
        Value::Array(items) => {
            for item in items {
                scan_image_response_strings(filter, item)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for item in map.values() {
                scan_image_response_strings(filter, item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod image_provider_filter_tests {
    use super::*;
    use crate::provider_filter::{FilterHide, ProviderFilter, ProviderFilterConfig};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn read_request(stream: &mut tokio::net::TcpStream) -> Value {
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let count = stream.read(&mut buffer).await.unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(header_end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap();
                if bytes.len() >= header_end + 4 + length {
                    return serde_json::from_slice(&bytes[header_end + 4..header_end + 4 + length])
                        .unwrap();
                }
            }
        }
    }

    async fn image_fixture(reply: &'static [u8]) -> (String, tokio::task::JoinHandle<Value>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/images/generations",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", reply.len()).as_bytes()).await.unwrap();
            stream.write_all(reply).await.unwrap();
            request
        });
        (url, server)
    }

    #[tokio::test]
    async fn image_provider_filter_sends_the_rewritten_prompt_and_placeholder() {
        let filter = Arc::new(
            ProviderFilter::compile(&ProviderFilterConfig {
                enabled: true,
                hide: FilterHide {
                    words: vec!["secret".into()],
                    ..Default::default()
                },
                placeholder_images: true,
                ..Default::default()
            })
            .unwrap(),
        );
        let (url, server) = image_fixture(br#"{"data":[{"b64_json":"b2s="}]}"#).await;
        let result = image_provider_response(&reqwest::Client::new(), &url, &serde_json::json!({
            "model":"image-model", "prompt":"paint SECRET", "image":"data:image/png;base64,c2VjcmV0"
        }), "", Some(filter)).await;
        assert!(result.is_ok(), "{result:?}");
        let sent = server.await.unwrap();
        assert_eq!(sent["model"], "image-model");
        assert_eq!(sent["prompt"], "paint ca-kw-01");
        assert_eq!(sent["image"], "[image]");
    }

    #[tokio::test]
    async fn image_provider_filter_stops_a_response_before_delivering_it() {
        let filter = Arc::new(
            ProviderFilter::compile(&ProviderFilterConfig {
                enabled: true,
                phrases: vec!["secret".into()],
                ..Default::default()
            })
            .unwrap(),
        );
        let (url, server) = image_fixture(br#"{"data":[],"revised_prompt":"SECRET"}"#).await;
        let result = image_provider_response(
            &reqwest::Client::new(),
            &url,
            &serde_json::json!({"prompt":"paint sky"}),
            "",
            Some(filter.clone()),
        )
        .await;
        server.await.unwrap();
        assert!(filter.is_tripped());
        assert!(result.unwrap_err().contains("phrase-1"));
    }

    #[tokio::test]
    async fn image_provider_filter_stops_decoded_json_response_text() {
        let filter = Arc::new(
            ProviderFilter::compile(&ProviderFilterConfig {
                enabled: true,
                phrases: vec!["secret".into()],
                ..Default::default()
            })
            .unwrap(),
        );
        let (url, server) = image_fixture(br#"{"data":[],"revised_prompt":"\u0073ecret"}"#).await;
        let result = image_provider_response(
            &reqwest::Client::new(),
            &url,
            &serde_json::json!({"prompt":"paint sky"}),
            "",
            Some(filter.clone()),
        )
        .await;
        server.await.unwrap();
        assert!(filter.is_tripped());
        assert!(result.unwrap_err().contains("phrase-1"));
    }

    #[tokio::test]
    async fn image_provider_filter_stops_before_sending_a_blocked_prompt() {
        let filter = Arc::new(
            ProviderFilter::compile(&ProviderFilterConfig {
                enabled: true,
                phrases: vec!["secret".into()],
                ..Default::default()
            })
            .unwrap(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/images/generations",
            listener.local_addr().unwrap()
        );
        let result = image_provider_response(
            &reqwest::Client::new(),
            &url,
            &serde_json::json!({"prompt":"paint SECRET"}),
            "",
            Some(filter),
        )
        .await;
        assert!(result.unwrap_err().contains("phrase-1"));
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn image_provider_filter_interrupts_an_inflight_request() {
        let filter = Arc::new(
            ProviderFilter::compile(&ProviderFilterConfig {
                enabled: true,
                phrases: vec!["secret".into()],
                ..Default::default()
            })
            .unwrap(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/images/generations",
            listener.local_addr().unwrap()
        );
        let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_request(&mut stream).await;
            accepted_tx.send(()).unwrap();
            let mut byte = [0];
            let _ = stream.read(&mut byte).await;
        });
        let worker_filter = filter.clone();
        let mut worker = tokio::spawn(async move {
            image_provider_response(
                &reqwest::Client::new(),
                &url,
                &serde_json::json!({"prompt":"paint sky"}),
                "",
                Some(worker_filter),
            )
            .await
        });
        accepted_rx.await.unwrap();
        assert!(filter.admit_outbound(b"secret").is_err());
        let outcome = tokio::time::timeout(Duration::from_secs(1), &mut worker).await;
        if outcome.is_err() {
            worker.abort();
        }
        server.abort();
        assert!(
            outcome.is_ok(),
            "image request must stop when the process filter trips"
        );
        assert!(outcome.unwrap().unwrap().unwrap_err().contains("phrase-1"));
    }
}

#[cfg(test)]
mod job_tool_tests {
    use super::*;
    use serde_json::json;
    use tokio::io::AsyncWriteExt;

    #[cfg(unix)]
    #[tokio::test]
    async fn install_binary_resolves_ordinary_build_source_from_tool_cwd() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("build-output");
        std::fs::copy("/bin/echo", &source).unwrap();
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o755)).unwrap();
        let destination = temp.path().join("installed-tool");
        let result = execute_tool(
            "install_binary",
            &json!({"source":"build-output", "destination":destination}),
            temp.path().to_str().unwrap(),
            5,
        )
        .await;
        assert!(result.is_success(), "{}", result.model_text());
        assert!(destination.is_file());
        assert_eq!(
            std::fs::metadata(destination).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }

    #[tokio::test]
    async fn retired_recovery_turn_cannot_start_a_typed_file_mutation() {
        let profile = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(temp.path()).unwrap();
        let mut context = ToolContext::native_read(
            &workspace,
            5,
            ToolProfile::Main,
            crate::sandbox::SandboxMode::PolicyOnly,
            crate::jobs::JobOwner::new(None),
        );
        let session_id = uuid::Uuid::new_v4();
        let recovery = crate::recovery::RecoveryContext::new(
            std::sync::Arc::new(crate::recovery::RecoveryStore::new(profile.path())),
            session_id,
            uuid::Uuid::new_v4(),
            context.workspace_roots.clone(),
            "main",
            crate::session_access::SessionAccessCoordinator::new(profile.path())
                .try_writer(session_id)
                .unwrap(),
        );
        recovery.close_mutations();
        context.recovery = Some(recovery.clone());
        let path = temp.path().join("must-not-be-created.txt");
        let result = execute_tool_with_context(
            "write_file",
            &json!({"path": path, "content": "retired"}),
            &context,
        )
        .await;
        assert!(!result.is_success());
        assert!(result.model_text().contains("turn has retired"));
        assert!(!path.exists());
        assert!(!recovery.has_active_mutations());
    }

    #[tokio::test]
    async fn bare_typed_file_mutation_obeys_global_workspace_restore_exclusion() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(temp.path()).unwrap();
        let context = ToolContext::native_read(
            &workspace,
            5,
            ToolProfile::Main,
            crate::sandbox::SandboxMode::PolicyOnly,
            crate::jobs::JobOwner::new(None),
        );
        assert!(context.recovery.is_none());
        let other_profile = tempfile::tempdir().unwrap();
        let store = crate::recovery::RecoveryStore::new(other_profile.path());
        let restore = store
            .try_workspace_restore(&context.workspace_roots)
            .unwrap();
        let path = temp.path().join("bare.txt");
        let arguments = json!({"path": path, "content": "authorized bare edit"});
        let denied = execute_tool_with_context("write_file", &arguments, &context).await;
        assert!(!denied.is_success());
        assert!(denied
            .model_text()
            .contains("workspace activity admission failed"));
        assert!(!path.exists());
        drop(restore);
        let allowed = execute_tool_with_context("write_file", &arguments, &context).await;
        assert!(allowed.is_success(), "{}", allowed.model_text());
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "authorized bare edit"
        );
        assert!(!other_profile.path().join("recovery").exists());

        // An already authorized absolute edit can be outside the configured
        // recording scope. Exclude recovery of its actual directory as well.
        let outside = tempfile::tempdir().unwrap();
        let restore = store
            .try_workspace_restore(&[outside.path().to_path_buf()])
            .unwrap();
        let outside_path = outside.path().join("approved-absolute.txt");
        let outside_arguments = json!({"path": outside_path, "content": "absolute edit"});
        let denied = execute_tool_with_context("write_file", &outside_arguments, &context).await;
        assert!(!denied.is_success());
        assert!(!outside_path.exists());
        drop(restore);
        assert!(
            execute_tool_with_context("write_file", &outside_arguments, &context)
                .await
                .is_success()
        );
        assert_eq!(
            std::fs::read_to_string(outside_path).unwrap(),
            "absolute edit"
        );
    }

    #[tokio::test]
    async fn native_read_run_retains_only_the_admitted_per_call_output() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(temp.path()).unwrap();
        let mut context = ToolContext::native_read(
            &workspace,
            5,
            ToolProfile::Main,
            crate::sandbox::SandboxMode::PolicyOnly,
            crate::jobs::JobOwner::new(None),
        );
        context.job_execution_policy =
            crate::jobs::JobExecutionPolicy::default().with_output_limit(256);
        let mut calls = Vec::new();
        for index in 0..4 {
            let path = temp.path().join(format!("bounded-{index}.txt"));
            std::fs::write(&path, "文字".repeat(5000)).unwrap();
            calls.push(PreparedNativeRead {
                name: "read_file".into(),
                args: json!({"path":path}),
            });
        }
        let results = execute_native_read_run(&calls, &context).await;
        assert_eq!(results.len(), 4);
        assert!(results
            .iter()
            .all(|result| result.is_success() && result.model_text().len() <= 256));
        assert!(results.iter().all(|result| result
            .model_text()
            .contains("output truncated by automation budget")));
    }

    #[tokio::test]
    async fn native_read_run_preserves_order_and_rejects_mutation_or_oversized_runs() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(temp.path()).unwrap();
        let context = ToolContext::native_read(
            &workspace,
            5,
            ToolProfile::Main,
            crate::sandbox::SandboxMode::PolicyOnly,
            crate::jobs::JobOwner::new(None),
        );
        let mut reads = Vec::new();
        for index in 0..4 {
            let path = temp.path().join(format!("{index}.txt"));
            std::fs::write(&path, format!("marker-{index}\n")).unwrap();
            reads.push(PreparedNativeRead {
                name: "read_file".into(),
                args: json!({"path": path}),
            });
        }
        let results = execute_native_read_run(&reads, &context).await;
        assert_eq!(results.len(), reads.len());
        for (index, result) in results.iter().enumerate() {
            assert!(result.is_success());
            assert!(result.model_text().contains(&format!("marker-{index}")));
        }
        let forbidden = temp.path().join("must-not-exist");
        let mut mixed = reads[..1].to_vec();
        mixed.push(PreparedNativeRead {
            name: "write_file".into(),
            args: json!({"path": forbidden, "content": "bad"}),
        });
        assert!(execute_native_read_run(&mixed, &context)
            .await
            .iter()
            .all(|result| !result.is_success()));
        assert!(!forbidden.exists());
        reads.push(reads[0].clone());
        assert_eq!(execute_native_read_run(&reads, &context).await.len(), 5);
        assert!(execute_native_read_run(&reads, &context)
            .await
            .iter()
            .all(|result| !result.is_success()));
    }

    #[tokio::test]
    #[ignore = "explicit native read fixture timing; reports evidence without asserting disk timing"]
    async fn native_read_fixture_benchmark() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(temp.path()).unwrap();
        let context = ToolContext::native_read(
            &workspace,
            30,
            ToolProfile::Main,
            crate::sandbox::SandboxMode::PolicyOnly,
            crate::jobs::JobOwner::new(None),
        );
        let fixture =
            "bounded read benchmark line with stable text and no secret material\n".repeat(128_000);
        let mut reads = Vec::new();
        for index in 0..4 {
            let path = temp.path().join(format!("fixture-{index}.txt"));
            std::fs::write(&path, &fixture).unwrap();
            reads.push(PreparedNativeRead {
                name: "read_file".into(),
                args: json!({"path": path}),
            });
        }
        let expected = execute_native_read_run(&reads, &context).await;
        let mut serial = Vec::new();
        let mut concurrent = Vec::new();
        for iteration in 0..7 {
            if iteration % 2 == 0 {
                let start = std::time::Instant::now();
                let mut results = Vec::new();
                for read in &reads {
                    results.push(execute_tool_with_context(&read.name, &read.args, &context).await);
                }
                serial.push(start.elapsed().as_micros());
                assert!(results == expected);
                let start = std::time::Instant::now();
                let results = execute_native_read_run(&reads, &context).await;
                concurrent.push(start.elapsed().as_micros());
                assert!(results == expected);
            } else {
                let start = std::time::Instant::now();
                let results = execute_native_read_run(&reads, &context).await;
                concurrent.push(start.elapsed().as_micros());
                assert!(results == expected);
                let start = std::time::Instant::now();
                let mut results = Vec::new();
                for read in &reads {
                    results.push(execute_tool_with_context(&read.name, &read.args, &context).await);
                }
                serial.push(start.elapsed().as_micros());
                assert!(results == expected);
            }
        }
        serial.sort();
        concurrent.sort();
        println!("native_read_fixture: four {} byte files; seven warm alternating pairs; serial_p50_us={} serial_p95_us={} native_p50_us={} native_p95_us={} p50_speedup={:.3}", fixture.len(), serial[3], serial[6], concurrent[3], concurrent[6], serial[3] as f64 / concurrent[3] as f64);
    }

    fn query_fixture(root: &std::path::Path, pattern: &str, glob_query: bool) -> String {
        bounded_file_query(
            root.to_path_buf(),
            vec![root.to_path_buf()],
            if glob_query {
                FileQuery::Glob(glob::Pattern::new(pattern).unwrap())
            } else {
                FileQuery::Search(regex::Regex::new(pattern).unwrap())
            },
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            std::time::Instant::now() + Duration::from_secs(5),
        )
    }

    #[test]
    fn bounded_queries_report_result_and_file_limits_with_stable_order() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("b.txt"), "hit\n".repeat(120)).unwrap();
        std::fs::write(temp.path().join("a.txt"), "hit\n").unwrap();
        std::fs::write(
            temp.path().join("large.txt"),
            vec![b'x'; QUERY_MAX_FILE_BYTES + 1],
        )
        .unwrap();
        let output = query_fixture(temp.path(), "hit", false);
        assert!(output.lines().next().unwrap().contains("a.txt: 1: hit"));
        assert_eq!(
            output.lines().filter(|line| line.contains(": hit")).count(),
            QUERY_MAX_RESULTS
        );
        assert!(output.contains("Query limited: result budget"));
        assert!(output.len() <= QUERY_MAX_OUTPUT_BYTES);
        let large = query_fixture(&temp.path().join("large.txt"), "x", false);
        assert!(large.contains("skipped 1"));
    }

    #[test]
    fn bounded_queries_handle_cancellation_and_deadline_without_false_complete() {
        let temp = tempfile::tempdir().unwrap();
        let call = |cancelled, deadline| {
            bounded_file_query(
                temp.path().to_path_buf(),
                vec![temp.path().to_path_buf()],
                FileQuery::Search(regex::Regex::new("x").unwrap()),
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(cancelled)),
                deadline,
            )
        };
        assert!(
            call(true, std::time::Instant::now() + Duration::from_secs(1)).contains("cancelled")
        );
        assert!(call(false, std::time::Instant::now()).contains("Query limited: deadline"));
    }

    #[cfg(unix)]
    #[test]
    fn bounded_queries_never_follow_directory_or_file_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "outside-hit").unwrap();
        std::fs::write(temp.path().join("inside.txt"), "inside-hit").unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join("escape-dir")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            temp.path().join("escape.txt"),
        )
        .unwrap();
        let search = query_fixture(temp.path(), "hit", false);
        assert!(search.contains("inside-hit"));
        assert!(!search.contains("outside-hit"));
        let glob = query_fixture(temp.path(), "**/*.txt", true);
        assert!(glob.contains("inside.txt"));
        assert!(!glob.contains("secret.txt"));
        assert!(!glob.contains("escape.txt"));
    }

    fn write_test_png(path: &std::path::Path, width: u32, height: u32) {
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            width,
            height,
            image::Rgba([12, 34, 56, 255]),
        ))
        .save(path)
        .unwrap();
    }

    #[test]
    fn configured_subagent_route_preserves_the_canonical_provider_identity() {
        let mut config = crate::config::CaConfig::default();
        config.providers = std::collections::HashMap::from([
            (
                "deepseek".into(),
                crate::config::ProviderConfig {
                    google: None,
                    tool_schema_policy: Default::default(),
                    base_url: "https://api.deepseek.com/v1".into(),
                    api_key: String::new(),
                    protocol: crate::agent::ProviderProtocol::ChatCompletions,
                    prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                    models: vec!["deepseek-model".into()],
                    timeout: crate::config::ProviderTimeoutConfig::default(),
                },
            ),
            (
                "openai".into(),
                crate::config::ProviderConfig {
                    google: None,
                    tool_schema_policy: Default::default(),
                    base_url: "https://api.openai.com/v1".into(),
                    api_key: String::new(),
                    protocol: crate::agent::ProviderProtocol::OpenAiResponses,
                    prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                    models: vec!["openai-model".into()],
                    timeout: crate::config::ProviderTimeoutConfig::default(),
                },
            ),
        ]);
        config.defaults.provider = "deepseek".into();
        config.defaults.model = "deepseek-model".into();

        let (provider_id, provider, model) = resolve_subagent_route(&config, None);
        let client = crate::agent::AgentClient::with_request_store(
            provider_id,
            &provider,
            &model,
            crate::request::RequestStore::default(),
        );
        assert_eq!(client.provider_id(), "deepseek");
        assert_eq!(client.model_id(), "deepseek-model");

        let (provider_id, provider, model) = resolve_subagent_route(&config, Some("openai"));
        let client = crate::agent::AgentClient::with_request_store(
            provider_id,
            &provider,
            &model,
            crate::request::RequestStore::default(),
        );
        assert_eq!(client.provider_id(), "openai");
        assert_eq!(client.model_id(), "openai-model");
    }

    #[tokio::test]
    async fn explicit_task_route_without_manager_fails_before_provider_resolution() {
        let temp = tempfile::tempdir().unwrap();
        let result = execute_tool(
            "task",
            &json!({
                "subagent_name": "explore",
                "prompt": "inspect this project",
                "route": {"provider": "openai-codex", "model": "configured-model"}
            }),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(
            matches!(result, ToolExecutionResult::Failure(ref message)
            if message == "Error: agent manager context is unavailable"),
            "{}",
            result.model_text()
        );
    }

    #[tokio::test]
    async fn view_image_normalizes_with_safe_process_only_output() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("fixture.png");
        write_test_png(&path, 3, 2);
        let result = execute_tool(
            "view_image",
            &json!({"path":"fixture.png"}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        let ToolExecutionResult::SuccessImage { marker, image } = result else {
            panic!("expected image result");
        };
        assert_eq!((image.width(), image.height()), (3, 2));
        assert!(marker.contains("image/png 3×2"));
        assert!(!marker.contains("fixture.png"));
        assert!(!format!("{image:?}").contains(&image.base64()));
    }

    #[tokio::test]
    async fn view_image_rejects_unsafe_types_and_limits_but_allows_external_reads() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("not-image.txt"), b"hello").unwrap();
        let unsupported = execute_tool(
            "view_image",
            &json!({"path":"not-image.txt"}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(unsupported, ToolExecutionResult::Failure(_)));

        std::fs::write(
            temp.path().join("oversized.png"),
            vec![0; crate::image_content::MAX_COMPRESSED_IMAGE_BYTES + 1],
        )
        .unwrap();
        let oversized = execute_tool(
            "view_image",
            &json!({"path":"oversized.png"}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(oversized, ToolExecutionResult::Failure(_)));

        let mismatch_source = temp.path().join("mismatch-source.png");
        write_test_png(&mismatch_source, 1, 1);
        std::fs::copy(&mismatch_source, temp.path().join("mismatch.jpg")).unwrap();
        let mismatch = execute_tool(
            "view_image",
            &json!({"path":"mismatch.jpg"}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(mismatch, ToolExecutionResult::Failure(_)));

        let parent_image = temp.path().parent().unwrap().join(format!(
            "outside-mm067-{}.png",
            temp.path().file_name().unwrap().to_string_lossy()
        ));
        write_test_png(&parent_image, 1, 1);
        let symlink = temp.path().join("external-link.png");
        std::os::unix::fs::symlink(&parent_image, &symlink).unwrap();
        let linked = execute_tool(
            "view_image",
            &json!({"path":"external-link.png"}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(linked.is_success(), "{linked:?}");
        let escaped = execute_tool(
            "view_image",
            &json!({"path":format!("../{}", parent_image.file_name().unwrap().to_string_lossy())}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(escaped, ToolExecutionResult::Failure(_)));
        std::fs::remove_file(parent_image).unwrap();
    }

    fn fixture_pdf(page_texts: &[&str], encrypted: bool, embedded: Option<&str>) -> Vec<u8> {
        use pdf_extract::content::{Content, Operation};
        use pdf_extract::{dictionary, Document, Object, Stream};

        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let resources_id = document.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let mut page_ids = Vec::new();
        for text in page_texts {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 12.into()]),
                    Operation::new("Td", vec![72.into(), 720.into()]),
                    Operation::new("Tj", vec![Object::string_literal(*text)]),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id =
                document.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            });
            page_ids.push(page_id.into());
        }
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => page_ids,
                "Count" => page_texts.len() as i64,
            }),
        );
        let mut catalog = dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        };
        if let Some(payload) = embedded {
            let stream_id = document.add_object(Stream::new(
                dictionary! { "Type" => "EmbeddedFile" },
                payload.as_bytes().to_vec(),
            ));
            let file_id = document.add_object(dictionary! {
                "Type" => "Filespec",
                "F" => Object::string_literal("payload.sh"),
                "EF" => dictionary! { "F" => stream_id },
            });
            catalog.set(
                "Names",
                dictionary! {
                    "EmbeddedFiles" => dictionary! {
                        "Names" => vec![Object::string_literal("payload.sh"), file_id.into()],
                    },
                },
            );
        }
        let catalog_id = document.add_object(catalog);
        document.trailer.set("Root", catalog_id);
        if encrypted {
            let encrypt_id = document.add_object(dictionary! {
                "Filter" => "Standard",
                "V" => 1,
                "R" => 2,
                "Length" => 40,
                "O" => Object::string_literal("owner"),
                "U" => Object::string_literal("user"),
                "P" => -4,
            });
            document.trailer.set("Encrypt", encrypt_id);
        }
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        bytes
    }

    async fn serve_web_response(
        status: &str,
        content_type: &str,
        body: Vec<u8>,
        chunked: bool,
        delay: Duration,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_string();
        let content_type = content_type.to_string();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let _ = socket.read(&mut request).await;
            tokio::time::sleep(delay).await;
            if chunked {
                let header = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
                );
                if socket.write_all(header.as_bytes()).await.is_err() {
                    return;
                }
                for chunk in body.chunks(16 * 1024) {
                    let prefix = format!("{:X}\r\n", chunk.len());
                    if socket.write_all(prefix.as_bytes()).await.is_err()
                        || socket.write_all(chunk).await.is_err()
                        || socket.write_all(b"\r\n").await.is_err()
                    {
                        return;
                    }
                }
                let _ = socket.write_all(b"0\r\n\r\n").await;
            } else {
                let header = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                if socket.write_all(header.as_bytes()).await.is_ok() {
                    let _ = socket.write_all(&body).await;
                }
            }
        });
        (format!("http://{address}/page"), handle)
    }

    #[test]
    fn job_tool_schemas_are_registered_and_background_is_optional() {
        let tools = default_tool_defs();
        for name in [
            "run_command",
            "await_job",
            "write_shell_stdin",
            "kill_job",
            "watch_process",
            "stop_watcher",
        ] {
            assert!(tools.iter().any(|tool| tool.function.name == name));
        }
        let run = tools
            .iter()
            .find(|tool| tool.function.name == "run_command")
            .unwrap();
        assert_eq!(
            run.function.parameters["properties"]["background"]["type"],
            "boolean"
        );
        assert_eq!(run.function.parameters["required"], json!(["command"]));

        let await_job = tools
            .iter()
            .find(|tool| tool.function.name == "await_job")
            .unwrap();
        assert_eq!(
            await_job.function.parameters["properties"]["stop"]["type"],
            "boolean"
        );
        assert_eq!(
            await_job.function.parameters["properties"]["grace_secs"]["type"],
            "integer"
        );
        assert_eq!(await_job.function.parameters["required"], json!(["job_id"]));

        let read = tools
            .iter()
            .find(|tool| tool.function.name == "read_file")
            .unwrap();
        assert_eq!(read.function.parameters["required"], json!(["path"]));
        assert_eq!(
            read.function.parameters["properties"]["start_line"]["type"],
            "integer"
        );
        assert_eq!(
            read.function.parameters["properties"]["end_line"]["type"],
            "integer"
        );
        assert_eq!(
            read.function.parameters["properties"]["include_line_hashes"]["type"],
            "boolean"
        );

        let anchored = tools
            .iter()
            .find(|tool| tool.function.name == "hashline_edit")
            .unwrap();
        assert_eq!(
            anchored.function.parameters["required"],
            json!(["end_anchor", "path", "replacement", "start_anchor"])
        );

        let pdf = tools
            .iter()
            .find(|tool| tool.function.name == "read_pdf")
            .unwrap();
        assert_eq!(pdf.function.parameters["required"], json!(["path"]));
        assert_eq!(
            pdf.function.parameters["properties"]["start_page"]["type"],
            "integer"
        );
        assert_eq!(
            pdf.function.parameters["properties"]["end_page"]["type"],
            "integer"
        );
    }

    #[tokio::test]
    async fn read_pdf_defaults_to_one_page_and_supports_inclusive_ranges() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("guide.pdf");
        std::fs::write(
            &path,
            fixture_pdf(&["FIRST PAGE", "SECOND PAGE", "THIRD PAGE"], false, None),
        )
        .unwrap();

        let defaulted = execute_tool(
            "read_pdf",
            &json!({"path": path}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(defaulted, ToolExecutionResult::Success(_)));
        assert!(defaulted.model_text().contains("FIRST PAGE"));
        assert!(!defaulted.model_text().contains("SECOND PAGE"));
        assert!(defaulted.model_text().contains("Showing pages 1-1 of 3"));
        assert!(defaulted.model_text().contains("\"start_page\":2"));

        let ranged = execute_tool(
            "read_pdf",
            &json!({"path": path, "start_page": 2, "end_page": 3}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(ranged, ToolExecutionResult::Success(_)));
        assert!(!ranged.model_text().contains("FIRST PAGE"));
        assert!(ranged.model_text().contains("SECOND PAGE"));
        assert!(ranged.model_text().contains("THIRD PAGE"));
        assert!(ranged.model_text().contains("Showing pages 2-3 of 3"));
        assert!(ranged
            .model_text()
            .contains("Omitted pages: 1 before, 0 after"));
    }

    #[tokio::test]
    async fn read_pdf_rejects_invalid_ranges_encryption_malformed_and_oversized_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("guide.pdf");
        std::fs::write(&path, fixture_pdf(&["PAGE"], false, None)).unwrap();

        for arguments in [
            json!({"path": path, "start_page": 0}),
            json!({"path": path, "start_page": 2, "end_page": 1}),
            json!({"path": path, "start_page": 1, "end_page": 21}),
            json!({"path": path, "start_page": 2}),
        ] {
            let result =
                execute_tool("read_pdf", &arguments, temp.path().to_str().unwrap(), 2).await;
            assert!(matches!(result, ToolExecutionResult::Failure(_)));
        }

        let encrypted = temp.path().join("encrypted.pdf");
        std::fs::write(&encrypted, fixture_pdf(&["SECRET"], true, None)).unwrap();
        let result = execute_tool(
            "read_pdf",
            &json!({"path": encrypted}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(result, ToolExecutionResult::Failure(_)));
        assert!(result
            .model_text()
            .contains("encrypted PDFs are not supported"));

        let malformed = temp.path().join("malformed.pdf");
        std::fs::write(&malformed, b"%PDF-1.7\nnot a document").unwrap();
        let result = execute_tool(
            "read_pdf",
            &json!({"path": malformed}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(result, ToolExecutionResult::Failure(_)));
        assert!(result.model_text().contains("malformed or unsupported"));

        let oversized = temp.path().join("oversized.pdf");
        let file = std::fs::File::create(&oversized).unwrap();
        file.set_len(PDF_MAX_FILE_BYTES + 1).unwrap();
        let result = execute_tool(
            "read_pdf",
            &json!({"path": oversized}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(result, ToolExecutionResult::Failure(_)));
        assert!(result.model_text().contains("file limit"));

        let wrong_extension = temp.path().join("document.bin");
        std::fs::write(&wrong_extension, fixture_pdf(&["PAGE"], false, None)).unwrap();
        let result = execute_tool(
            "read_pdf",
            &json!({"path": wrong_extension}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(result, ToolExecutionResult::Failure(_)));
        assert!(result.model_text().contains(".pdf file"));

        let too_many_pages = temp.path().join("too-many-pages.pdf");
        let mut document =
            pdf_extract::Document::load_mem(&fixture_pdf(&["PAGE"], false, None)).unwrap();
        let page_id = *document.get_pages().values().next().unwrap();
        let pages_id = document
            .catalog()
            .unwrap()
            .get(b"Pages")
            .unwrap()
            .as_reference()
            .unwrap();
        let page = document.get_object(page_id).unwrap().clone();
        let mut page_ids = vec![pdf_extract::Object::Reference(page_id)];
        for _ in 1..=PDF_MAX_PAGES {
            page_ids.push(pdf_extract::Object::Reference(
                document.add_object(page.clone()),
            ));
        }
        let pages = document
            .get_object_mut(pages_id)
            .unwrap()
            .as_dict_mut()
            .unwrap();
        pages.set("Kids", page_ids);
        pages.set("Count", (PDF_MAX_PAGES + 1) as i64);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        std::fs::write(&too_many_pages, bytes).unwrap();
        let result = execute_tool(
            "read_pdf",
            &json!({"path": too_many_pages}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(result, ToolExecutionResult::Failure(_)));
        assert!(result.model_text().contains("page document limit"));
    }

    #[tokio::test]
    async fn read_pdf_contains_parser_panics_and_allows_external_reads() {
        use pdf_extract::Document;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("missing-media-box.pdf");
        let bytes = fixture_pdf(&["PAGE"], false, None);
        let mut document = Document::load_mem(&bytes).unwrap();
        let page_id = *document.get_pages().values().next().unwrap();
        document
            .get_object_mut(page_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .remove(b"MediaBox");
        let mut broken = Vec::new();
        document.save_to(&mut broken).unwrap();
        std::fs::write(&path, broken).unwrap();
        let result = execute_tool(
            "read_pdf",
            &json!({"path": path}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(result, ToolExecutionResult::Failure(_)));
        assert!(result.model_text().contains("malformed or unsupported"));
        assert!(!result
            .model_text()
            .contains(&path.to_string_lossy().to_string()));

        let outside = tempfile::tempdir().unwrap();
        let outside_path = outside.path().join("outside.pdf");
        std::fs::write(&outside_path, fixture_pdf(&["OUTSIDE"], false, None)).unwrap();
        let result = execute_tool(
            "read_pdf",
            &json!({"path": outside_path}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(result.is_success(), "{result:?}");
        assert!(result.model_text().contains("OUTSIDE"));
    }

    #[tokio::test]
    async fn read_pdf_bounds_text_sanitizes_controls_and_ignores_embedded_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("bounded.pdf");
        let large = format!("BEGIN\u{1b}[31m{}END", "x".repeat(PDF_TEXT_CAP_BYTES * 2));
        std::fs::write(
            &path,
            fixture_pdf(&[&large], false, Some("EMBEDDED_NEVER_EXTRACTED")),
        )
        .unwrap();
        let result = execute_tool(
            "read_pdf",
            &json!({"path": path}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(result, ToolExecutionResult::Success(_)));
        let output = result.model_text();
        assert!(output.contains("BEGIN"));
        assert!(!output.contains('\u{1b}'));
        assert!(!output.contains("EMBEDDED_NEVER_EXTRACTED"));
        assert!(output.contains("remaining selected-page text was not extracted"));
        assert!(output.len() <= PDF_MAX_RESULT_BYTES);
    }

    #[tokio::test]
    async fn read_file_ranges_unicode_crlf_and_reports_the_next_range() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("unicode.txt");
        let content = (1..=205)
            .map(|line| format!("行{line}\r\n"))
            .collect::<String>();
        std::fs::write(&path, content.as_bytes()).unwrap();

        let ranged = execute_tool(
            "read_file",
            &json!({"path": path, "start_line": 2, "end_line": 3}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(ranged, ToolExecutionResult::Success(_)));
        let output = ranged.model_text();
        assert!(output.starts_with("行2\n行3\n"), "{output}");
        assert!(output.contains("Showing lines 2-3 of 205"), "{output}");
        assert!(output.contains("Omitted lines: 1 before, 202 after"));
        assert!(output.contains("\"start_line\":4"));

        let defaulted = execute_tool(
            "read_file",
            &json!({"path": path}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(defaulted
            .model_text()
            .contains("Showing lines 1-200 of 205"));
        assert!(defaulted.model_text().contains("\"start_line\":201"));
    }

    #[tokio::test]
    async fn hashline_read_and_edit_preserve_unicode_crlf_receipts_and_reject_stale_anchors() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("anchored.txt");
        let original = "α\r\nβeta\r\nγ\r\nlast";
        std::fs::write(&path, original).unwrap();
        let beta = line_anchor("βeta");
        let gamma = line_anchor("γ");

        let read = execute_tool(
            "read_file",
            &json!({
                "path": path,
                "start_line": 1,
                "end_line": 4,
                "include_line_hashes": true
            }),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(read.is_success(), "{read:?}");
        assert!(read
            .model_text()
            .contains(&format!("{} α", line_anchor("α"))));
        assert!(read.model_text().contains(&format!("{beta} βeta")));
        assert!(read.model_text().contains(&format!("{gamma} γ")));

        let edited = execute_tool(
            "hashline_edit",
            &json!({
                "path": path,
                "start_anchor": beta,
                "end_anchor": gamma,
                "replacement": "BETA\r\nΓAMMA"
            }),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        let receipt = edited.mutation().expect("anchored edit receipt");
        assert_eq!(receipt.action, "hashline_edit");
        assert_eq!(
            receipt.before,
            crate::undo::FileState::Utf8(original.into())
        );
        assert_eq!(
            receipt.after,
            crate::undo::FileState::Utf8("α\r\nBETA\r\nΓAMMA\r\nlast".into())
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "α\r\nBETA\r\nΓAMMA\r\nlast"
        );

        let stale = execute_tool(
            "hashline_edit",
            &json!({
                "path": path,
                "start_anchor": line_anchor("βeta"),
                "end_anchor": line_anchor("γ"),
                "replacement": "must not write"
            }),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(stale, ToolExecutionResult::Failure(_)));
        assert!(stale.model_text().contains("stale or missing"));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "α\r\nBETA\r\nΓAMMA\r\nlast"
        );
    }

    #[test]
    fn hashline_edit_rejects_duplicate_reversed_and_malformed_anchors_without_changes() {
        let duplicate = "same\nsame\ntail\n";
        let error = apply_hashline_edit(
            duplicate,
            &line_anchor("same"),
            &line_anchor("tail"),
            "replacement",
        )
        .unwrap_err();
        assert!(error.contains("ambiguous"));
        let reversed = apply_hashline_edit(
            "first\nsecond\n",
            &line_anchor("second"),
            &line_anchor("first"),
            "replacement",
        )
        .unwrap_err();
        assert!(reversed.contains("reversed or noncontiguous"));
        let malformed =
            apply_hashline_edit("first\n", "xyz", "12345678", "replacement").unwrap_err();
        assert!(malformed.contains("eight hexadecimal"));
    }

    #[tokio::test]
    async fn read_file_rejects_oversized_spans_invalid_utf8_and_bounds_long_lines() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("long.txt");
        let line = format!("HEAD{}TAIL\n", "界".repeat(30_000));
        std::fs::write(&path, line).unwrap();
        let output = execute_tool(
            "read_file",
            &json!({"path": path}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(output, ToolExecutionResult::Success(_)));
        assert!(output.model_text().contains("bytes omitted from line 1"));
        assert!(output.model_text().contains("HEAD"));
        assert!(output.model_text().contains("TAIL"));
        assert!(output.model_text().len() <= READ_FILE_MAX_RESULT_BYTES);

        let too_many = execute_tool(
            "read_file",
            &json!({"path": path, "start_line": 1, "end_line": 501}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(too_many, ToolExecutionResult::Failure(_)));
        assert!(too_many.model_text().contains("500-line maximum"));

        let binary = temp.path().join("binary.txt");
        std::fs::write(&binary, [0xff, 0xfe, b'\n']).unwrap();
        let invalid = execute_tool(
            "read_file",
            &json!({"path": binary}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(invalid, ToolExecutionResult::Failure(_)));
        assert!(invalid.model_text().contains("not valid UTF-8"));

        let many = temp.path().join("many.txt");
        let content = (0..500)
            .map(|line| format!("{line:03} {}\n", "z".repeat(1_000)))
            .collect::<String>();
        std::fs::write(&many, content).unwrap();
        let bounded = execute_tool(
            "read_file",
            &json!({"path": many, "start_line": 1, "end_line": 500}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(bounded, ToolExecutionResult::Success(_)));
        assert!(bounded.model_text().len() <= READ_FILE_MAX_RESULT_BYTES);
        assert!(bounded.model_text().contains("bytes omitted"));
        assert!(bounded.model_text().contains("Showing lines 1-500 of 500"));
    }

    #[tokio::test]
    async fn web_fetch_streams_chunked_text_through_cap_with_truthful_marker() {
        let mut body = b"<html><body>START ".to_vec();
        body.extend(std::iter::repeat_n(
            b'a',
            WEB_FETCH_MAX_DECODED_BYTES + 64 * 1024,
        ));
        body.extend_from_slice(b" END</body></html>");
        let (url, server) = serve_web_response(
            "200 OK",
            "text/html; charset=utf-8",
            body,
            true,
            Duration::ZERO,
        )
        .await;

        let output = web_fetch_bounded(&url, Duration::from_secs(2))
            .await
            .unwrap();
        server.await.unwrap();

        assert!(output.contains("START"));
        assert!(output.contains("remaining response was not read"));
        assert!(output.len() <= WEB_FETCH_MAX_RESULT_BYTES);
        assert!(!output.contains(" END"));
    }

    #[tokio::test]
    async fn web_fetch_rejects_status_binary_invalid_utf8_and_timeout_without_bodies() {
        let (url, server) = serve_web_response(
            "500 Internal Server Error",
            "text/plain",
            b"SECRET_RESPONSE_MARKER".to_vec(),
            false,
            Duration::ZERO,
        )
        .await;
        let error = web_fetch_bounded(&url, Duration::from_secs(2))
            .await
            .unwrap_err();
        server.await.unwrap();
        assert_eq!(error, "HTTP 500");
        assert!(!error.contains("SECRET_RESPONSE_MARKER"));

        let (url, server) = serve_web_response(
            "200 OK",
            "application/octet-stream",
            b"BINARY_BODY_MARKER".to_vec(),
            false,
            Duration::ZERO,
        )
        .await;
        let error = web_fetch_bounded(&url, Duration::from_secs(2))
            .await
            .unwrap_err();
        server.await.unwrap();
        assert_eq!(error, "unsupported content type application/octet-stream");
        assert!(!error.contains("BINARY_BODY_MARKER"));

        let (url, server) = serve_web_response(
            "200 OK",
            "text/plain",
            vec![0xff, 0xfe],
            false,
            Duration::ZERO,
        )
        .await;
        let error = web_fetch_bounded(&url, Duration::from_secs(2))
            .await
            .unwrap_err();
        server.await.unwrap();
        assert_eq!(error, "response is not valid UTF-8");

        let (url, server) = serve_web_response(
            "200 OK",
            "text/plain",
            b"late".to_vec(),
            false,
            Duration::from_millis(100),
        )
        .await;
        let error = web_fetch_bounded(&url, Duration::from_millis(20))
            .await
            .unwrap_err();
        server.await.unwrap();
        assert_eq!(error, "request timed out");
        assert!(!error.contains(&url));
    }

    #[test]
    fn generic_truncation_is_utf8_safe_and_reports_exact_omitted_bytes() {
        let source = format!("HEAD{}TAIL", "🙂".repeat(100));
        let output = truncate_output(&source, 100);
        assert!(output.len() <= 100);
        assert!(output.starts_with("HEAD"));
        assert!(output.ends_with("TAIL"));
        let mut lines = output.lines();
        let head = lines.next().unwrap();
        let marker = lines.next().unwrap();
        let tail = lines.next().unwrap();
        let omitted = source.len() - head.len() - tail.len();
        assert_eq!(marker, format!("[… {omitted} bytes omitted …]"));
    }

    #[tokio::test]
    async fn invalid_unknown_and_unavailable_calls_fail_before_execution() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("must-not-exist");
        let invalid = execute_tool(
            "write_file",
            &json!({"path": target}),
            temp.path().to_str().unwrap(),
            2,
        )
        .await;
        assert!(matches!(invalid, ToolExecutionResult::Failure(_)));
        assert!(!target.exists());

        let unknown = execute_tool("not_registered", &json!({}), "/tmp", 2).await;
        assert!(matches!(unknown, ToolExecutionResult::Failure(_)));

        let context = ToolContext {
            recovery: crate::owned_tools::current_recovery(),
            cwd: "/tmp".into(),
            timeout_secs: 2,
            profile: ToolProfile::Checkpoint,
            workspace_roots: vec!["/tmp".into()],
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            network_allowed: false,
            job_owner: crate::jobs::JobOwner::process(),
            job_execution_policy: crate::jobs::JobExecutionPolicy::default(),
        };
        let unavailable =
            execute_tool_with_context("read_file", &json!({"path":"Cargo.toml"}), &context).await;
        assert!(matches!(unavailable, ToolExecutionResult::Failure(_)));
    }

    #[tokio::test]
    async fn await_job_stop_arguments_fail_closed_before_job_lookup() {
        let context = ToolContext {
            recovery: crate::owned_tools::current_recovery(),
            cwd: "/tmp".into(),
            timeout_secs: 2,
            profile: ToolProfile::Main,
            workspace_roots: vec!["/tmp".into()],
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            network_allowed: false,
            job_owner: crate::jobs::JobOwner::new(None),
            job_execution_policy: crate::jobs::JobExecutionPolicy::default(),
        };
        let id = uuid::Uuid::new_v4().to_string();
        for (arguments, expected) in [
            (
                json!({"job_id":id, "stop":true, "timeout_secs":1}),
                "conflicts with timeout_secs",
            ),
            (json!({"job_id":id, "grace_secs":1}), "requires stop=true"),
            (
                json!({"job_id":id, "stop":true, "grace_secs":301}),
                "between 0 and 300",
            ),
        ] {
            let result = execute_tool_with_context("await_job", &arguments, &context).await;
            assert!(matches!(result, ToolExecutionResult::Failure(_)));
            assert!(result.model_text().contains(expected), "{result:?}");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn await_job_stop_returns_final_output_and_claims_completion_once() {
        let owner = crate::jobs::JobOwner::new(None);
        let manager = crate::jobs::JobManager::process();
        let id = manager
            .start_sandboxed_owned_with_origin(
                "trap 'printf tool-stopped; exit 0' TERM; while :; do sleep 1; done",
                "/tmp",
                true,
                Duration::from_secs(20),
                vec![std::fs::canonicalize("/tmp").unwrap()],
                false,
                crate::sandbox::SandboxMode::PolicyOnly,
                owner,
                crate::jobs::JobOrigin::ModelTool,
            )
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(40)).await;
        let context = ToolContext {
            recovery: crate::owned_tools::current_recovery(),
            cwd: "/tmp".into(),
            timeout_secs: 2,
            profile: ToolProfile::Main,
            workspace_roots: vec!["/tmp".into()],
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            network_allowed: false,
            job_owner: owner,
            job_execution_policy: crate::jobs::JobExecutionPolicy::default(),
        };
        let result = execute_tool_with_context(
            "await_job",
            &json!({"job_id":id.to_string(), "stop":true, "grace_secs":2}),
            &context,
        )
        .await;
        assert!(!result.is_success());
        assert!(result.model_text().contains("status: cancelled"));
        assert!(result.model_text().contains("tool-stopped"));
        assert!(manager
            .claim_completed_model_jobs_owned(owner)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn file_mutation_tools_return_exact_process_only_receipts() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("receipt.txt");
        let cwd = temp.path().to_str().unwrap();

        let created = execute_tool(
            "write_file",
            &json!({"path": target, "content": "alpha\n"}),
            cwd,
            2,
        )
        .await;
        let receipt = created.mutation().expect("write receipt");
        assert_eq!(receipt.before, crate::undo::FileState::Missing);
        assert_eq!(
            receipt.after,
            crate::undo::FileState::Utf8("alpha\n".into())
        );
        assert!(!created.model_text().contains("FileMutationReceipt"));

        let edited = execute_tool(
            "edit_file",
            &json!({"path": target, "old_string": "alpha", "new_string": "beta"}),
            cwd,
            2,
        )
        .await;
        let receipt = edited.mutation().expect("edit receipt");
        assert_eq!(
            receipt.before,
            crate::undo::FileState::Utf8("alpha\n".into())
        );
        assert_eq!(receipt.after, crate::undo::FileState::Utf8("beta\n".into()));

        let patched = execute_tool(
            "apply_patch",
            &json!({
                "path": target,
                "patch": "@@ -1 +1 @@\n-beta\n+gamma"
            }),
            cwd,
            2,
        )
        .await;
        let receipt = patched.mutation().expect("patch receipt");
        assert_eq!(
            receipt.before,
            crate::undo::FileState::Utf8("beta\n".into())
        );
        assert_eq!(
            receipt.after,
            crate::undo::FileState::Utf8("gamma\n".into())
        );

        let rejected = execute_tool(
            "apply_patch",
            &json!({
                "path": target,
                "patch": "@@ -1 +1 @@\n-missing\n+changed"
            }),
            cwd,
            2,
        )
        .await;
        assert!(matches!(rejected, ToolExecutionResult::Failure(_)));
        assert!(rejected.mutation().is_none());
        assert_eq!(tokio::fs::read_to_string(&target).await.unwrap(), "gamma\n");

        let deleted = execute_tool("delete_file", &json!({"path": target}), cwd, 2).await;
        let receipt = deleted.mutation().expect("delete receipt");
        assert_eq!(
            receipt.before,
            crate::undo::FileState::Utf8("gamma\n".into())
        );
        assert_eq!(receipt.after, crate::undo::FileState::Missing);
        assert!(!target.exists());
    }

    #[test]
    fn unified_patch_preserves_context_and_final_newline() {
        let original = "A\nold\nB\n";
        let patch = "@@ -1,3 +1,3 @@\n A\n-old\n+new\n B";
        assert_eq!(apply_unified_patch(original, patch).unwrap(), "A\nnew\nB\n");
    }

    #[test]
    fn unified_patch_applies_multiple_hunks_with_exact_unique_relocation() {
        let original = "zero\none\ntwo\nthree\nfour\nfive\n";
        let patch = "--- ignored/a.txt\n+++ ignored/b.txt\n@@ -1,2 +1,2 @@\n zero\n-one\n+ONE\n@@ -7,2 +7,2 @@\n four\n-five\n+FIVE";
        assert_eq!(
            apply_unified_patch(original, patch).unwrap(),
            "zero\nONE\ntwo\nthree\nfour\nFIVE\n"
        );
    }

    #[test]
    fn unified_patch_rejects_ambiguous_or_mismatched_hunks_without_source_echo() {
        let ambiguous = apply_unified_patch(
            "same\nold\nsame\nold\n",
            "@@ -9,2 +9,2 @@\n same\n-old\n+new",
        )
        .unwrap_err();
        assert!(ambiguous.contains("ambiguous"));
        assert!(!ambiguous.contains("same"));

        let mismatch =
            apply_unified_patch("alpha\nbeta\n", "@@ -1,2 +1,2 @@\n alpha\n-wrong\n+gamma")
                .unwrap_err();
        assert!(mismatch.contains("file unchanged"));
        assert!(!mismatch.contains("wrong"));
    }

    #[test]
    fn unified_patch_validates_counts_and_preserves_crlf_and_no_newline_marker() {
        let bad_counts =
            apply_unified_patch("alpha\nbeta\n", "@@ -1,3 +1,2 @@\n alpha\n-beta\n+gamma")
                .unwrap_err();
        assert!(bad_counts.contains("count"));

        let crlf = apply_unified_patch(
            "alpha\r\nbeta",
            "@@ -1,2 +1,2 @@\n alpha\n-beta\n\\ No newline at end of file\n+gamma\n\\ No newline at end of file",
        )
        .unwrap();
        assert_eq!(crlf, "alpha\r\ngamma");
    }

    #[test]
    fn unified_patch_rejects_file_operations_and_overlapping_hunks() {
        for patch in [
            "--- /dev/null\n+++ b/new\n@@ -0,0 +1 @@\n+new",
            "rename from old\nrename to new\n@@ -1 +1 @@\n-old\n+new",
            "GIT binary patch\n",
        ] {
            assert!(apply_unified_patch("old\n", patch).is_err());
        }

        let overlap = "@@ -1,2 +1,2 @@\n-a\n+A\n b\n@@ -2,1 +2,1 @@\n-b\n+B";
        assert!(apply_unified_patch("a\nb\n", overlap)
            .unwrap_err()
            .contains("overlap"));

        let oversized = "x".repeat(MAX_UNIFIED_PATCH_BYTES + 1);
        let error = apply_unified_patch("stable\n", &oversized).unwrap_err();
        assert!(error.contains("bounded input limit"));
        assert!(error.contains("file unchanged"));
    }

    #[tokio::test]
    async fn failed_or_over_bound_file_mutations_emit_no_receipt_and_do_not_write() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("bounded.txt");
        std::fs::write(&target, "stable").unwrap();
        let cwd = temp.path().to_str().unwrap();

        let failed = execute_tool(
            "edit_file",
            &json!({"path": target, "old_string": "missing", "new_string": "changed"}),
            cwd,
            2,
        )
        .await;
        assert!(matches!(failed, ToolExecutionResult::Failure(_)));
        assert!(failed.mutation().is_none());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "stable");

        let oversized = "x".repeat(crate::undo::MAX_UNDO_FILE_BYTES + 1);
        let rejected = execute_tool(
            "write_file",
            &json!({"path": target, "content": oversized}),
            cwd,
            2,
        )
        .await;
        assert!(matches!(rejected, ToolExecutionResult::Failure(_)));
        assert!(rejected.mutation().is_none());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "stable");
    }

    #[tokio::test]
    async fn prepared_effective_network_authority_is_not_rechecked_from_raw_arguments() {
        if !std::path::Path::new("/usr/bin/ssh").is_file() {
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(root.path()).unwrap();
        let prepared = crate::workspace::prepare_tool_invocation(
            &workspace,
            "run_command",
            &json!({"command":"/usr/bin/ssh -V","cwd":root.path()}),
            crate::agent::registry::ToolOrigin::BuiltIn,
        )
        .unwrap();
        assert!(prepared.network_requested);
        assert!(prepared.arguments.get("network").is_none());
        let context = ToolContext {
            recovery: crate::owned_tools::current_recovery(),
            cwd: root.path().to_path_buf(),
            timeout_secs: 2,
            profile: ToolProfile::Main,
            workspace_roots: vec![root.path().to_path_buf()],
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            network_allowed: true,
            job_owner: crate::jobs::JobOwner::new(None),
            job_execution_policy: crate::jobs::JobExecutionPolicy::default(),
        };
        let result = execute_tool_with_context("run_command", &prepared.arguments, &context).await;
        assert!(result.is_success(), "{}", result.model_text());
        assert!(result
            .command_observation()
            .is_some_and(|observation| observation.network_requested));
    }

    #[tokio::test]
    async fn read_lints_runs_as_an_owned_managed_offline_job() {
        if std::process::Command::new("cargo")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname = \"lint_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join("src/lib.rs"),
            "pub fn answer() -> u8 { 42 }\n",
        )
        .unwrap();
        let context = ToolContext {
            recovery: crate::owned_tools::current_recovery(),
            cwd: root.path().to_path_buf(),
            timeout_secs: 30,
            profile: ToolProfile::Main,
            workspace_roots: vec![root.path().to_path_buf()],
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            network_allowed: false,
            job_owner: crate::jobs::JobOwner::new(None),
            job_execution_policy: crate::jobs::JobExecutionPolicy::default(),
        };
        let result =
            execute_tool_with_context("read_lints", &json!({"path":root.path()}), &context).await;
        assert!(result.is_success(), "{}", result.model_text());
        let observation = result.command_observation().expect("managed job receipt");
        assert_eq!(observation.command, "cargo check --message-format=short");
        assert!(!observation.network_requested);
    }

    #[tokio::test]
    async fn await_job_timeout_returns_running_status_instead_of_hanging() {
        let root = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(root.path()).unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        let started = execute_tool_in_workspace_policy_owned(
            "run_command",
            &json!({"command": "sleep 30", "background": true}),
            &workspace,
            2,
            crate::sandbox::SandboxMode::PolicyOnly,
            false,
            owner,
        )
        .await;
        assert!(started.is_success(), "{}", started.model_text());
        let id = started
            .model_text()
            .strip_prefix("Started background job ")
            .expect("background job marker")
            .to_string();

        let began = std::time::Instant::now();
        let awaited = execute_tool_in_workspace_policy_owned(
            "await_job",
            &json!({"job_id": id, "timeout_secs": 2}),
            &workspace,
            2,
            crate::sandbox::SandboxMode::PolicyOnly,
            false,
            owner,
        )
        .await;
        let waited = began.elapsed();

        assert!(
            waited < std::time::Duration::from_secs(8),
            "await_job must honor a short timeout; waited {waited:?}"
        );
        assert!(
            awaited
                .model_text()
                .to_ascii_lowercase()
                .contains("running"),
            "expected a still-running snapshot, got: {}",
            awaited.model_text()
        );
        crate::jobs::JobManager::process()
            .stop_and_await_owned(
                id.parse().unwrap(),
                owner,
                std::time::Duration::from_secs(2),
            )
            .await
            .unwrap();
        assert!(crate::command_mutations::finish(owner)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn await_job_omitted_timeout_uses_short_default_not_forever() {
        let root = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(root.path()).unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        let started = execute_tool_in_workspace_policy_owned(
            "run_command",
            &json!({"command": "sleep 300", "background": true}),
            &workspace,
            2,
            crate::sandbox::SandboxMode::PolicyOnly,
            false,
            owner,
        )
        .await;
        assert!(started.is_success(), "{}", started.model_text());
        let id = started
            .model_text()
            .strip_prefix("Started background job ")
            .expect("background job marker")
            .to_string();

        let began = std::time::Instant::now();
        let awaited = execute_tool_in_workspace_policy_owned(
            "await_job",
            &json!({"job_id": id}),
            &workspace,
            2,
            crate::sandbox::SandboxMode::PolicyOnly,
            false,
            owner,
        )
        .await;
        let waited = began.elapsed();

        // DEFAULT_AWAIT_JOB_SECS is 30; must not wait for the 300s job.
        assert!(
            waited >= std::time::Duration::from_secs(20),
            "omitted timeout should use the short default, not return instantly: {waited:?}"
        );
        assert!(
            waited < std::time::Duration::from_secs(45),
            "await_job must not block for the full job lifetime; waited {waited:?}"
        );
        assert!(
            awaited
                .model_text()
                .to_ascii_lowercase()
                .contains("running"),
            "expected a still-running snapshot, got: {}",
            awaited.model_text()
        );
        crate::jobs::JobManager::process()
            .stop_and_await_owned(
                id.parse().unwrap(),
                owner,
                std::time::Duration::from_secs(2),
            )
            .await
            .unwrap();
        assert!(crate::command_mutations::finish(owner)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn background_tool_returns_a_stable_awaitable_job_id() {
        let root = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(root.path()).unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        let output = execute_tool_in_workspace_policy_owned(
            "run_command",
            &json!({"command": "printf background-ok", "background": true}),
            &workspace,
            2,
            crate::sandbox::SandboxMode::PolicyOnly,
            false,
            owner,
        )
        .await;
        assert!(output.is_success(), "{}", output.model_text());
        let id = output
            .model_text()
            .strip_prefix("Started background job ")
            .expect("background job marker");
        let awaited = execute_tool_in_workspace_policy_owned(
            "await_job",
            &json!({"job_id": id, "timeout_secs": 2}),
            &workspace,
            2,
            crate::sandbox::SandboxMode::PolicyOnly,
            false,
            owner,
        )
        .await;
        assert!(
            awaited.model_text().contains("background-ok"),
            "{}",
            awaited.model_text()
        );
        assert!(crate::command_mutations::finish(owner)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn oversized_workspace_file_does_not_block_an_unrelated_command() {
        let root = tempfile::tempdir().unwrap();
        std::fs::File::create(root.path().join("oversized.txt"))
            .unwrap()
            .set_len(64 * 1024 * 1024 + 1)
            .unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(root.path()).unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        let result = execute_tool_in_workspace_policy_owned(
            "run_command",
            &json!({"command": "printf must-not-run > marker"}),
            &workspace,
            2,
            crate::sandbox::SandboxMode::PolicyOnly,
            false,
            owner,
        )
        .await;

        assert!(result.is_success(), "{}", result.model_text());
        assert!(root.path().join("marker").is_file());
        let batches = crate::command_mutations::finish(owner).await.unwrap();
        assert!(batches
            .iter()
            .flat_map(|batch| batch.receipts())
            .any(|file| file.path.ends_with("marker")));
        assert!(batches
            .iter()
            .flat_map(|batch| batch.receipts())
            .all(|file| !file.path.ends_with("oversized.txt")));
        assert!(!crate::command_mutations::has_pending(owner));
    }

    #[tokio::test]
    async fn watcher_tools_register_and_stop_without_network_or_target_control() {
        let root = tempfile::tempdir().unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        let context = ToolContext {
            recovery: crate::owned_tools::current_recovery(),
            cwd: root.path().to_path_buf(),
            timeout_secs: 2,
            profile: ToolProfile::Main,
            workspace_roots: vec![root.path().to_path_buf()],
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            network_allowed: false,
            job_owner: owner,
            job_execution_policy: crate::jobs::JobExecutionPolicy::default(),
        };
        let started = execute_tool_with_context(
            "watch_process",
            &json!({
                "label":"current test",
                "host":{"type":"local"},
                "pid":std::process::id(),
                "process":{"type":"process"},
                "interval_secs":2
            }),
            &context,
        )
        .await;
        let id: ca_watch::WatchId = started
            .model_text()
            .strip_prefix("Started process watcher ")
            .expect("watcher marker")
            .parse()
            .expect("watcher id");
        assert!(crate::process_watch::ProcessWatchManager::process()
            .list_owned(owner)
            .await
            .iter()
            .any(|watcher| watcher.watch_id == id));

        let stopped = execute_tool_with_context(
            "stop_watcher",
            &json!({"watch_id":id.to_string()}),
            &context,
        )
        .await;
        assert!(stopped.is_success(), "{}", stopped.model_text());
        assert!(stopped.model_text().contains("target process unaffected"));

        let remote = execute_tool_with_context(
            "watch_process",
            &json!({
                "label":"remote",
                "host":{"type":"ssh","destination":"root@bsd"},
                "pid":42,
                "process":{"type":"process"},
                "interval_secs":10
            }),
            &context,
        )
        .await;
        assert!(matches!(remote, ToolExecutionResult::Failure(_)));
        assert!(remote.model_text().contains("approved network access"));
    }

    #[tokio::test]
    async fn foreground_job_keeps_non_process_owner_and_nonzero_is_failure() {
        let temp = tempfile::tempdir().unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        let context = ToolContext {
            recovery: crate::owned_tools::current_recovery(),
            cwd: temp.path().to_path_buf(),
            timeout_secs: 2,
            profile: ToolProfile::Main,
            workspace_roots: vec![temp.path().to_path_buf()],
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            network_allowed: false,
            job_owner: owner,
            job_execution_policy: crate::jobs::JobExecutionPolicy::default(),
        };
        let success = execute_tool_with_context(
            "run_command",
            &json!({"command": "printf owner-ok"}),
            &context,
        )
        .await;
        assert!(matches!(
            success,
            ToolExecutionResult::Command { success: true, .. }
        ));
        assert!(success.model_text().contains("owner-ok"));
        assert!(!success.model_text().contains("different run or session"));

        let failed = execute_tool_with_context(
            "run_command",
            &json!({"command": "printf failed >&2; exit 7"}),
            &context,
        )
        .await;
        assert!(matches!(
            failed,
            ToolExecutionResult::Command { success: false, .. }
        ));
        assert!(failed.model_text().contains("exit code: 7"));
        assert!(failed.model_text().contains("failed"));
    }

    #[test]
    fn terminal_job_statuses_map_to_typed_tool_outcomes() {
        let job = |status| crate::jobs::JobInfo {
            id: crate::jobs::JobId::new(),
            command: "command".into(),
            cwd: "/tmp".into(),
            status,
            elapsed: Duration::ZERO,
            stdout: String::new(),
            stderr: String::new(),
            stdout_omitted_bytes: 0,
            stderr_omitted_bytes: 0,
            background: false,
        };
        assert!(matches!(
            classify_job_result(
                job(crate::jobs::JobStatus::Completed { exit_code: 0 }),
                false
            ),
            ToolExecutionResult::Command { success: true, .. }
        ));
        assert!(matches!(
            classify_job_result(job(crate::jobs::JobStatus::TimedOut), false),
            ToolExecutionResult::Command { success: false, .. }
        ));
        assert!(matches!(
            classify_job_result(job(crate::jobs::JobStatus::Cancelled), false),
            ToolExecutionResult::Command { success: false, .. }
        ));
        assert!(matches!(
            classify_job_result(job(crate::jobs::JobStatus::Running), false),
            ToolExecutionResult::Command { success: true, .. }
        ));
        assert!(matches!(
            classify_job_result(job(crate::jobs::JobStatus::Cancelling), false),
            ToolExecutionResult::Command { success: true, .. }
        ));
    }

    #[tokio::test]
    async fn exact_skill_activation_uses_the_injected_registry() {
        let temp = tempfile::tempdir().unwrap();
        let skill_dir = temp.path().join("skills/deploy");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: deploy\ndescription: Deploy\n---\nbody",
        )
        .unwrap();
        let skills = std::sync::Arc::new(std::sync::Mutex::new(
            crate::skills::SkillRegistry::discover(
                crate::skills::SkillRoots {
                    project: Some(temp.path().join("skills")),
                    project_boundary: Some(temp.path().to_path_buf()),
                    user: None,
                    compatibility: Vec::new(),
                },
                &[],
            ),
        ));
        let workspace = crate::workspace::WorkspaceContext::discover(temp.path()).unwrap();

        let result = execute_prepared_tool_in_workspace_policy_owned_with_skills(
            "activate_skill",
            &json!({"id":"deploy"}),
            &workspace,
            2,
            crate::sandbox::SandboxMode::PolicyOnly,
            false,
            crate::jobs::JobOwner::process(),
            &skills,
        )
        .await;
        assert!(result.is_success(), "{}", result.model_text());
        assert!(skills.lock().unwrap().is_active("deploy"));

        let missing = execute_prepared_tool_in_workspace_policy_owned_with_skills(
            "activate_skill",
            &json!({"id":"missing"}),
            &workspace,
            2,
            crate::sandbox::SandboxMode::PolicyOnly,
            false,
            crate::jobs::JobOwner::process(),
            &skills,
        )
        .await;
        assert!(matches!(missing, ToolExecutionResult::Failure(_)));
    }

    #[tokio::test]
    async fn memory_tool_returns_only_a_process_local_validated_proposal() {
        let temp = tempfile::tempdir().unwrap();
        let context = ToolContext {
            recovery: crate::owned_tools::current_recovery(),
            cwd: temp.path().to_path_buf(),
            timeout_secs: 2,
            profile: ToolProfile::Main,
            workspace_roots: vec![temp.path().to_path_buf()],
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            network_allowed: false,
            job_owner: crate::jobs::JobOwner::process(),
            job_execution_policy: crate::jobs::JobExecutionPolicy::default(),
        };
        let proposed = execute_tool_with_context(
            "propose_memory",
            &json!({
                "scope":"workspace",
                "kind":"convention",
                "text":"Use hermetic fixtures for provider checks."
            }),
            &context,
        )
        .await;
        assert!(matches!(
            proposed,
            ToolExecutionResult::MemoryProposal { .. }
        ));
        assert!(proposed.model_text().contains("explicit review"));
        assert!(proposed.memory_proposal().is_some());
        assert!(temp.path().read_dir().unwrap().next().is_none());

        let rejected = execute_tool_with_context(
            "propose_memory",
            &json!({"scope":"user", "kind":"preference", "text":"api_key=secret"}),
            &context,
        )
        .await;
        assert!(matches!(rejected, ToolExecutionResult::Failure(_)));
        assert!(!rejected.model_text().contains("secret"));
    }
}
