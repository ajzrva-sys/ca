//! Explicit built-in privacy contracts. A new handler must choose a contract.
use crate::agent::registry::ToolHandler;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolPrivacy {
    Local,
    SourceEdit,
    LiteralCommand,
    FilenamePredicate,
    PrivateBatch,
    ModelPayload,
    PrivilegedArgv,
    Opaque,
}
impl ToolPrivacy {
    pub const fn for_handler(handler: ToolHandler) -> Self {
        use ToolHandler::*;
        match handler {
            Mcp => Self::Opaque,
            RunCommand => Self::LiteralCommand,
            WriteFile | EditFile | SearchReplace | ApplyPatch | DeleteFile | HashlineEdit
            | UpdateTodos => Self::SourceEdit,
            SearchFiles | Glob => Self::FilenamePredicate,
            SelectFiles | MoveFiles => Self::PrivateBatch,
            GenerateImage => Self::ModelPayload,
            RunPrivileged => Self::PrivilegedArgv,
            Infer
            | ReadFile
            | ReadPdf
            | ViewImage
            | Browser
            | BrowserExec
            | BrowserVault
            | ListDir
            | AwaitJob
            | WriteShellStdin
            | KillJob
            | WatchProcess
            | StopWatcher
            | SessionSearch
            | MemorySearch
            | WebSearch
            | WebFetch
            | AskChatGpt
            | ReadLints
            | ReadTodos
            | ReadScratch
            | CreatePlan
            | ProposePlan
            | PlanWorkflow
            | ConfigureAgentGraph
            | ProposeMemory
            | ProposeAutomation
            | ListLocalAutomations
            | EditPlan
            | UpdatePlanTask
            | RecordPlanEvidence
            | RequestUserInput
            | RequestWriteAccess
            | AskQuestion
            | CreateDirectory
            | SwitchMode
            | SendMessage
            | Fetch
            | Blame
            | SwitchBranch
            | InstallBinary
            | Task
            | SpawnAgent
            | SpawnAgentBatch
            | RequestGraphClarification
            | AwaitAgent
            | AwaitAgentBatch
            | SendAgentMessage
            | CancelAgent
            | SetAgentSpawning
            | CancelAgentJob
            | ListAgents
            | ListAgentRoutes
            | ActivateSkill
            | SkillSearch
            | SkillView
            | SkillManage
            | DelegateMechanistic
            | TransparentMechanistic
            | RemoteMechanistic
            | SearchMcpTools
            | SearchTools
            | UseTool
            | LspSymbols
            | LspDefinition
            | LspReferences
            | LspHover
            | LspImplementation
            | LspCallHierarchy
            | ReportReviewFindings
            | EmailAccounts
            | EmailList
            | EmailRead
            | EmailDraft
            | EmailSend
            | EmailReply
            | HttpRequest
            | GitOperation
            | SshOperation
            | ExecuteCode
            | TodoWrite
            | UpdateGoal
            | EnterPlanMode
            | ExitPlanMode
            | Lsp
            | SchedulerCreate
            | SchedulerList
            | SchedulerDelete
            | Monitor
            | ImageGen
            | ImageEdit
            | ImageToVideo
            | ReferenceToVideo => Self::Local,
        }
    }
    pub const fn callable(self) -> bool {
        !matches!(self, Self::Opaque)
    }
}

/// Resource fields not represented by the original top-level resource list.
pub(super) fn restore_extra_paths(
    context: &super::Context,
    name: &str,
    args: &mut serde_json::Value,
) -> Result<(), String> {
    fn path(
        context: &super::Context,
        value: &mut serde_json::Value,
        create: bool,
        relative: bool,
    ) -> Result<(), String> {
        if let Some(items) = value.as_array_mut() {
            for item in items {
                path(context, item, create, relative)?;
            }
            return Ok(());
        }
        let Some(text) = value.as_str() else {
            return Ok(());
        };
        if url::Url::parse(text).is_ok_and(|url| matches!(url.scheme(), "http" | "https")) {
            return Ok(());
        }
        let _ = (context, create, relative, text);
        Ok(())
    }
    if name == "generate_image" {
        if let Some(filename) = args["filename"].as_str().map(str::to_owned) {
            let native = if filename.starts_with("__CA_") {
                context.resolve_filename_path(&filename, true)?
            } else {
                if std::path::Path::new(&filename).is_absolute()
                    || std::path::Path::new(&filename)
                        .components()
                        .any(|component| matches!(component, std::path::Component::ParentDir))
                {
                    return Err(
                        "Image output needs a mapped destination or public relative filename"
                            .into(),
                    );
                }
                let directory = context.workspace().join(".ca-images");
                let parent = if directory.is_dir() {
                    context.project_filename_path(&directory)?
                } else {
                    format!(
                        "{}/.ca-images",
                        context.project_filename_path(context.workspace())?
                    )
                };
                context.resolve_filename_path(&format!("{parent}/{filename}"), true)?
            };
            args["filename"] = serde_json::Value::String(
                native.to_str().ok_or("Unsupported image filename")?.into(),
            );
        }
    }
    if name == "spawn_agent" {
        if let Some(value) = args
            .get_mut("repair_contract")
            .and_then(|v| v.get_mut("path"))
        {
            path(context, value, false, false)?;
        }
        restore_repair_checks(context, args)?;
    }
    if name == "spawn_agent_batch" {
        if let Some(tasks) = args
            .get_mut("tasks")
            .and_then(serde_json::Value::as_array_mut)
        {
            for task in tasks {
                if let Some(value) = task
                    .get_mut("repair_contract")
                    .and_then(|v| v.get_mut("path"))
                {
                    path(context, value, false, false)?;
                }
                restore_repair_checks(context, task)?;
            }
        }
    }
    let fields: &[(&str, bool, bool)] = match name {
        "install_binary" => &[("source", false, false), ("destination", true, false)],
        "request_write_access" => &[("path", false, false)],
        "email_draft" | "email_reply" => &[("attachments", false, false)],
        "inspect_symbol" | "inspect_feature" | "find_relevant_tests" | "verify_changes" => {
            &[("paths", false, true)]
        }
        "verify_recipe" => &[("cwd", false, false)],
        "generate_image" => &[("image_url", false, false)],
        _ => &[],
    };
    for (field, create, relative) in fields {
        if let Some(value) = args.get_mut(*field) {
            if name == "request_write_access" {
                if let Some(reference) = value.as_str().filter(|text| text.starts_with("__CA_")) {
                    *value = serde_json::Value::String(
                        context
                            .resolve_filename_grant_path(reference)?
                            .to_string_lossy()
                            .into_owned(),
                    );
                }
            } else {
                path(context, value, *create, *relative)?;
            }
        }
    }
    fn nested(context: &super::Context, value: &mut serde_json::Value) -> Result<(), String> {
        match value {
            serde_json::Value::Object(items) => {
                for (key, value) in items {
                    if matches!(
                        key.as_str(),
                        "output_schema" | "schema" | "route" | "tools" | "tool_allowlist"
                    ) {
                        continue;
                    }
                    if matches!(
                        key.as_str(),
                        "path"
                            | "paths"
                            | "cwd"
                            | "assigned_files"
                            | "attachments"
                            | "log_path"
                            | "completion_marker"
                    ) {
                        path(context, value, false, false)?;
                    } else {
                        nested(context, value)?;
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    nested(context, item)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    // Top-level fields were already resolved with their tool-specific roles.
    // Walk only nested containers, where the old resource list has no entries.
    if let Some(items) = args.as_object_mut() {
        for (key, value) in items {
            if !matches!(
                key.as_str(),
                "output_schema" | "schema" | "route" | "tools" | "tool_allowlist"
            ) && (value.is_object() || value.is_array())
            {
                if matches!(key.as_str(), "assigned_files" | "attachments" | "paths") {
                    path(context, value, false, false)?;
                } else {
                    nested(context, value)?;
                }
            }
        }
    }
    Ok(())
}

/// A repair check already declares closed argv. Restore only the corresponding
/// exact grant, then let normal child admission enforce its command authority.
fn restore_repair_checks(
    context: &super::Context,
    args: &mut serde_json::Value,
) -> Result<(), String> {
    let Some(value) = args.get("repair_contract").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let mut contract: crate::narrow_worker::RepairContract = serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid repair_contract: {error}"))?;
    if contract.checks.len() > 8 {
        return Err("invalid bounded repair checks".into());
    }
    let original_commands: Vec<String> = serde_json::from_value(args["commands"].clone())
        .map_err(|_| "repair checks require exact command grants".to_string())?;
    let mut commands = original_commands.clone();
    let restore = |text: &str| -> Result<String, String> { context.restore_text(text) };
    for check in &mut contract.checks {
        let original = crate::narrow_worker::render_command(&check.argv);
        if !original_commands.contains(&original) {
            return Err("pinned repair check is outside its exact command grant".into());
        }
        for argument in &mut check.argv {
            *argument = restore(argument)?;
        }
        if let Some(cwd) = &mut check.cwd {
            *cwd = restore(cwd)?;
        }
        let native = crate::narrow_worker::render_command(&check.argv);
        for (command, original_command) in commands.iter_mut().zip(&original_commands) {
            if original_command == &original {
                *command = native.clone();
            }
        }
    }
    contract.validate()?;
    args["repair_contract"]["checks"] =
        serde_json::to_value(&contract.checks).map_err(|error| error.to_string())?;
    args["commands"] = serde_json::Value::from(commands);
    Ok(())
}

pub(super) fn restore_local_payload(
    context: &super::Context,
    value: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    use serde_json::Value;
    match value {
        Value::String(text) => context.restore_text(text).map(Value::String),
        Value::Array(items) => items
            .iter()
            .map(|value| restore_local_payload(context, value))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(items) => {
            let mut restored = serde_json::Map::new();
            for (key, value) in items {
                // These subtrees are interface/control contracts, not data.
                let value = if matches!(
                    key.as_str(),
                    "output_schema"
                        | "schema"
                        | "route"
                        | "tools"
                        | "tool_allowlist"
                        | "expected_revision"
                        | "program"
                ) {
                    value.clone()
                } else if key == "command" || key == "commands" {
                    let restore = |text: &str| -> Result<Value, String> {
                        let text = super::shell::restore(text, |token| {
                            context
                                .state
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .registry
                                .restore_exact(token)
                                .map(str::to_owned)
                        })?;
                        super::shell::restore_filename_references(&text, |reference| {
                            context
                                .resolve_filename_path(reference, true)
                                .map(|path| path.to_string_lossy().into_owned())
                        })
                        .map(Value::String)
                    };
                    match value {
                        Value::String(text) => restore(text)?,
                        Value::Array(values) => Value::Array(
                            values
                                .iter()
                                .map(|value| {
                                    value
                                        .as_str()
                                        .ok_or_else(|| "Command scope must be a string".to_string())
                                        .and_then(restore)
                                })
                                .collect::<Result<Vec<_>, _>>()?,
                        ),
                        _ => value.clone(),
                    }
                } else {
                    restore_local_payload(context, value)?
                };
                restored.insert(key.clone(), value);
            }
            Ok(Value::Object(restored))
        }
        _ => Ok(value.clone()),
    }
}

pub(crate) fn needs_preparation(name: &str, value: &serde_json::Value) -> bool {
    fn contains(value: &serde_json::Value, skip_pattern: bool, model: bool) -> bool {
        match value {
            serde_json::Value::String(text) => text.contains("__CA_PRIVATE_"),
            serde_json::Value::Array(values) => values
                .iter()
                .any(|value| contains(value, skip_pattern, model)),
            serde_json::Value::Object(values) => values.iter().any(|(key, value)| {
                !matches!(
                    key.as_str(),
                    "output_schema" | "schema" | "route" | "tools" | "tool_allowlist"
                ) && !(skip_pattern && key == "pattern")
                    && !(model && key == "prompt")
                    && !key.starts_with("_ca_")
                    && contains(value, skip_pattern, model)
            }),
            _ => false,
        }
    }
    contains(
        value,
        matches!(name, "glob" | "search_files"),
        name == "generate_image",
    )
}

/// Extra write resources whose original schemas do not declare a top-level
/// ResourceKind::Path. Child grants must cover them as well as ordinary paths.
pub(crate) fn additional_write_paths(
    name: &str,
    args: &serde_json::Value,
    cwd: &std::path::Path,
) -> Result<Vec<std::path::PathBuf>, String> {
    let native = |value: &str| {
        if std::path::Path::new(value).is_absolute() {
            std::path::PathBuf::from(value)
        } else {
            cwd.join(value)
        }
    };
    match name {
        "generate_image" => Ok(vec![args["filename"].as_str().map_or_else(
            || cwd.join(".ca-images"),
            |filename| {
                if std::path::Path::new(filename).is_absolute() {
                    native(filename)
                } else {
                    cwd.join(".ca-images").join(filename)
                }
            },
        )]),
        "install_binary" => Ok(vec![args["destination"]
            .as_str()
            .map(native)
            .map(Ok)
            .unwrap_or_else(|| {
                std::env::current_exe()
                    .map_err(|_| "Installation destination is unavailable".to_string())
            })?]),
        "move_files" => super::tasks::move_authority_paths(args),
        "switch_branch" => Ok(vec![cwd.to_path_buf()]),
        _ => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_builtin_privacy_contracts_are_explicit_and_callable() {
        let registry = crate::agent::registry::ToolRegistry::builtins();
        assert_eq!(
            registry.specs().len(),
            112,
            "update the reviewed tool coverage inventory"
        );
        for spec in registry.specs() {
            assert_eq!(spec.privacy, ToolPrivacy::for_handler(spec.handler));
            assert!(spec.privacy.callable(), "{}", spec.name);
        }
        assert!(!ToolPrivacy::for_handler(ToolHandler::Mcp).callable());
    }
}
