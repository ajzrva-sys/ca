use serde_json::Value;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RootId(Uuid);

impl RootId {
    fn for_path(path: &Path) -> Self {
        Self(Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            path.to_string_lossy().as_bytes(),
        ))
    }
}

impl std::fmt::Display for RootId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for RootId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRoot {
    pub id: RootId,
    pub path: PathBuf,
    pub read_only: bool,
    pub primary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathOperation {
    Read,
    WriteExisting,
    Create,
}

#[derive(Debug, Clone)]
pub struct WorkspaceContext {
    roots: Vec<WorkspaceRoot>,
    cwd: PathBuf,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum WorkspaceError {
    #[error("workspace path does not exist: {0}")]
    Missing(String),
    #[error("invalid tool arguments: {0}")]
    InvalidArguments(String),
    #[error("workspace path contains traversal: {0}")]
    Traversal(String),
    #[error("model path is outside workspace roots: {0}")]
    OutsideRoots(String),
    #[error("workspace root is read-only: {0}")]
    ReadOnly(String),
    #[error("workspace has no roots")]
    NoRoots,
}

impl WorkspaceContext {
    pub fn uninitialized() -> Self {
        Self {
            roots: Vec::new(),
            cwd: PathBuf::new(),
        }
    }

    pub fn discover(launch_cwd: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let launch = canonical_directory(launch_cwd.as_ref())?;
        let primary = git_top_level(&launch).unwrap_or_else(|| launch.clone());
        Ok(Self::from_primary(primary, launch))
    }

    pub fn from_primary(primary: PathBuf, cwd: PathBuf) -> Self {
        let root = WorkspaceRoot {
            id: RootId::for_path(&primary),
            path: primary,
            read_only: false,
            primary: true,
        };
        Self {
            roots: vec![root],
            cwd,
        }
    }

    pub fn from_roots(primary: &Path, additional: &[PathBuf]) -> Result<Self, WorkspaceError> {
        let primary = canonical_directory(primary)?;
        let mut workspace = Self::from_primary(primary.clone(), primary);
        for path in additional {
            workspace.add_root(path, false)?;
        }
        Ok(workspace)
    }

    pub fn roots(&self) -> &[WorkspaceRoot] {
        &self.roots
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn primary(&self) -> Option<&WorkspaceRoot> {
        self.roots.iter().find(|root| root.primary)
    }

    pub fn skill_ancestor_roots(&self) -> Vec<PathBuf> {
        let Some(primary) = self.primary() else {
            return Vec::new();
        };
        if !self.cwd.starts_with(&primary.path) {
            return Vec::new();
        }
        self.cwd
            .ancestors()
            .take_while(|ancestor| ancestor.starts_with(&primary.path))
            .map(|ancestor| ancestor.join(".agents/skills"))
            .collect()
    }

    pub fn add_root(
        &mut self,
        path: impl AsRef<Path>,
        read_only: bool,
    ) -> Result<RootId, WorkspaceError> {
        let path = canonical_directory(path.as_ref())?;
        if let Some(root) = self.roots.iter().find(|root| root.path == path) {
            return Ok(root.id);
        }
        let id = RootId::for_path(&path);
        self.roots.push(WorkspaceRoot {
            id,
            path,
            read_only,
            primary: false,
        });
        Ok(id)
    }

    /// Replace only the primary root while preserving supplemental roots in
    /// their original order. Launch-created worktrees use this before any
    /// workspace-dependent registry is constructed.
    pub fn replace_primary(&mut self, path: impl AsRef<Path>) -> Result<RootId, WorkspaceError> {
        let path = canonical_directory(path.as_ref())?;
        let id = RootId::for_path(&path);
        let mut roots = vec![WorkspaceRoot {
            id,
            path: path.clone(),
            read_only: false,
            primary: true,
        }];
        roots.extend(
            self.roots
                .iter()
                .filter(|root| !root.primary && root.path != path)
                .cloned(),
        );
        self.roots = roots;
        self.cwd = path;
        Ok(id)
    }

    pub fn remove_root(&mut self, id: RootId) -> bool {
        let Some(index) = self
            .roots
            .iter()
            .position(|root| root.id == id && !root.primary)
        else {
            return false;
        };
        if self.cwd.starts_with(&self.roots[index].path) {
            if let Some(primary) = self.primary() {
                self.cwd = primary.path.clone();
            }
        }
        self.roots.remove(index);
        true
    }

    pub fn set_cwd(&mut self, path: &str) -> Result<(), WorkspaceError> {
        let resolved = self.resolve(path, PathOperation::Read)?;
        if self.root_relative(&resolved).is_none() {
            return Err(WorkspaceError::OutsideRoots(resolved.display().to_string()));
        }
        if !resolved.is_dir() {
            return Err(WorkspaceError::Missing(path.into()));
        }
        self.cwd = resolved;
        Ok(())
    }

    pub fn resolve(&self, raw: &str, operation: PathOperation) -> Result<PathBuf, WorkspaceError> {
        if self.roots.is_empty() {
            return Err(WorkspaceError::NoRoots);
        }
        let path = Path::new(raw);
        if raw.starts_with('~')
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(WorkspaceError::Traversal(raw.into()));
        }
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        };
        let resolved = match operation {
            PathOperation::Read | PathOperation::WriteExisting => canonical_existing(&candidate)?,
            PathOperation::Create => canonical_creation(&candidate)?,
        };
        if operation == PathOperation::Read {
            return Ok(resolved);
        }
        let root = self
            .roots
            .iter()
            .find(|root| resolved.starts_with(&root.path))
            .ok_or_else(|| WorkspaceError::OutsideRoots(resolved.display().to_string()))?;
        if operation != PathOperation::Read && root.read_only {
            return Err(WorkspaceError::ReadOnly(root.path.display().to_string()));
        }
        Ok(resolved)
    }

    pub fn root_relative(&self, path: &Path) -> Option<(RootId, PathBuf)> {
        self.roots.iter().find_map(|root| {
            path.strip_prefix(&root.path)
                .ok()
                .map(|relative| (root.id, relative.to_path_buf()))
        })
    }

    pub fn persisted_additional_roots(&self) -> Vec<String> {
        self.roots
            .iter()
            .filter(|root| !root.primary)
            .map(|root| root.path.to_string_lossy().into_owned())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedToolInvocation {
    pub name: String,
    pub arguments: Value,
    pub origin: crate::agent::registry::ToolOrigin,
    pub network_requested: bool,
}

pub fn prepare_tool_invocation(
    workspace: &WorkspaceContext,
    name: &str,
    arguments: &Value,
    origin: crate::agent::registry::ToolOrigin,
) -> Result<PreparedToolInvocation, WorkspaceError> {
    use crate::agent::registry::{ResourceKind, ToolHandler};
    let registry = crate::agent::registry::ToolRegistry::builtins();
    let spec = registry
        .get(name)
        .ok_or_else(|| WorkspaceError::InvalidArguments(format!("unknown tool: {name}")))?;
    registry
        .validate(name, arguments)
        .map_err(|error| WorkspaceError::InvalidArguments(error.to_string()))?;
    if spec.handler == ToolHandler::Task {
        let subagent_name = arguments
            .get("subagent_name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !crate::agent::subagent::BUILTIN_SUBAGENT_NAMES.contains(&subagent_name) {
            return Err(WorkspaceError::InvalidArguments(format!(
                "unknown subagent: {subagent_name}; available subagents: {}",
                crate::agent::subagent::BUILTIN_SUBAGENT_NAMES.join(", ")
            )));
        }
    }
    let mut prepared = arguments.clone();
    let object = prepared.as_object_mut().ok_or_else(|| {
        WorkspaceError::InvalidArguments("tool arguments are not an object".into())
    })?;
    for resource in &spec.resources {
        let operation = match resource.kind {
            ResourceKind::WorkingDirectory => PathOperation::Read,
            ResourceKind::Path => match (spec.handler, resource.field) {
                (
                    ToolHandler::WriteFile
                    | ToolHandler::CreateDirectory
                    | ToolHandler::Fetch
                    | ToolHandler::Browser,
                    _,
                ) => PathOperation::Create,
                (
                    ToolHandler::EditFile
                    | ToolHandler::HashlineEdit
                    | ToolHandler::ApplyPatch
                    | ToolHandler::DeleteFile
                    | ToolHandler::UpdateTodos,
                    _,
                ) => PathOperation::WriteExisting,
                _ => PathOperation::Read,
            },
            _ => continue,
        };
        let raw = object
            .get(resource.field)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                let defaults_to_cwd = resource.kind == ResourceKind::WorkingDirectory
                    || (resource.kind == ResourceKind::Path
                        && !resource.required
                        && matches!(
                            spec.handler,
                            ToolHandler::SearchFiles
                                | ToolHandler::Glob
                                | ToolHandler::ReadLints
                                | ToolHandler::ReadTodos
                        ));
                defaults_to_cwd.then(|| workspace.cwd().to_string_lossy().into_owned())
            });
        let Some(raw) = raw else {
            continue;
        };
        let resolved = workspace.resolve(&raw, operation)?;
        object.insert(
            resource.field.into(),
            Value::String(resolved.to_string_lossy().into_owned()),
        );
    }
    let browser_network = matches!(spec.handler, ToolHandler::Browser)
        && matches!(
            object.get("action").and_then(Value::as_str),
            Some("navigate" | "download")
        );
    let command_network = matches!(spec.handler, ToolHandler::RunCommand)
        && object
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(obvious_network_client_command);
    let watcher_network = matches!(spec.handler, ToolHandler::WatchProcess)
        && object
            .get("host")
            .and_then(Value::as_object)
            .and_then(|host| host.get("type"))
            .and_then(Value::as_str)
            .is_some_and(|host| host == "ssh");
    let workflow_preview_network = name == "preview_automation_workflow"
        && !matches!(object.get("depth").and_then(Value::as_str), Some("offline"));
    let network_requested = browser_network
        || command_network
        || watcher_network
        || workflow_preview_network
        || spec
            .capabilities
            .contains(&crate::agent::registry::ToolCapability::Network)
        || object
            .get("network")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    Ok(PreparedToolInvocation {
        name: name.into(),
        arguments: prepared,
        origin,
        network_requested,
    })
}

fn obvious_network_client_command(command: &str) -> bool {
    shell_command_stages(command)
        .into_iter()
        .any(|stage| stage_requests_network(&stage))
}

/// Split a shell command into simple stages at unquoted `;`, `&&`, `||`, and
/// `|` so compound invocations such as `cd repo && git push` still classify.
fn shell_command_stages(command: &str) -> Vec<String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }
    let mut stages = Vec::new();
    let mut current = String::new();
    let mut quote = Quote::None;
    let mut escaped = false;
    for character in command.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        match quote {
            Quote::Single => {
                if character == '\'' {
                    quote = Quote::None;
                }
                current.push(character);
            }
            Quote::Double => match character {
                '"' => {
                    quote = Quote::None;
                    current.push(character);
                }
                '\\' => {
                    escaped = true;
                    current.push(character);
                }
                _ => current.push(character),
            },
            Quote::None => match character {
                '\'' => {
                    quote = Quote::Single;
                    current.push(character);
                }
                '"' => {
                    quote = Quote::Double;
                    current.push(character);
                }
                '\\' => {
                    escaped = true;
                    current.push(character);
                }
                ';' | '|' | '&' => {
                    if !current.trim().is_empty() {
                        stages.push(std::mem::take(&mut current));
                    } else {
                        current.clear();
                    }
                    continue;
                }
                _ => current.push(character),
            },
        }
    }
    if !current.trim().is_empty() {
        stages.push(current);
    }
    stages
}

fn stage_requests_network(stage: &str) -> bool {
    let Some(program) = stage.split_ascii_whitespace().next() else {
        return false;
    };
    if program.bytes().any(|byte| {
        matches!(
            byte,
            b'\'' | b'"' | b'\\' | b'$' | b'`' | b';' | b'|' | b'&'
        )
    }) {
        return false;
    }
    let path = Path::new(program);
    let name = path.file_name().and_then(|name| name.to_str());
    if !trusted_network_client_spelling(path) {
        return false;
    }
    match name {
        Some("ssh" | "scp" | "sftp") => true,
        // GitHub CLI talks to api.github.com for nearly every subcommand.
        Some("gh") => true,
        Some("curl" | "wget" | "rsync") => true,
        Some("git") => git_subcommand_requests_network(stage),
        _ => false,
    }
}

fn trusted_network_client_spelling(path: &Path) -> bool {
    if path.components().count() == 1 {
        return true;
    }
    path.is_absolute()
        && matches!(
            path.parent().and_then(Path::to_str),
            Some(
                "/bin"
                    | "/usr/bin"
                    | "/sbin"
                    | "/usr/sbin"
                    | "/opt/homebrew/bin"
                    | "/opt/homebrew/sbin"
                    | "/usr/local/bin"
                    | "/usr/local/sbin"
            )
        )
}

fn git_subcommand_requests_network(command: &str) -> bool {
    let mut words = command.split_ascii_whitespace();
    let _program = words.next();
    let mut expect_path = false;
    for word in words {
        if expect_path {
            expect_path = false;
            continue;
        }
        match word {
            "-C" => expect_path = true,
            word if word.starts_with('-') => continue,
            word => return matches!(word, "push" | "fetch" | "pull" | "clone" | "ls-remote"),
        }
    }
    false
}

fn canonical_existing(path: &Path) -> Result<PathBuf, WorkspaceError> {
    std::fs::canonicalize(path).map_err(|_| WorkspaceError::Missing(path.display().to_string()))
}

fn canonical_directory(path: &Path) -> Result<PathBuf, WorkspaceError> {
    let canonical = canonical_existing(path)?;
    if !canonical.is_dir() {
        return Err(WorkspaceError::Missing(path.display().to_string()));
    }
    Ok(canonical)
}

fn canonical_creation(path: &Path) -> Result<PathBuf, WorkspaceError> {
    let mut ancestor = path;
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        let name = ancestor
            .file_name()
            .ok_or_else(|| WorkspaceError::Missing(path.display().to_string()))?;
        suffix.push(name.to_os_string());
        ancestor = ancestor
            .parent()
            .ok_or_else(|| WorkspaceError::Missing(path.display().to_string()))?;
    }
    let mut resolved = canonical_existing(ancestor)?;
    for component in suffix.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn git_top_level(cwd: &Path) -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    canonical_existing(Path::new(path.trim())).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_reads_may_leave_workspace_but_writes_and_traversal_remain_confined() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("inside.txt"), "ok").unwrap();
        std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
        let workspace = WorkspaceContext::from_primary(
            std::fs::canonicalize(root.path()).unwrap(),
            std::fs::canonicalize(root.path()).unwrap(),
        );
        assert!(matches!(
            workspace.resolve("../escape", PathOperation::Create),
            Err(WorkspaceError::Traversal(_))
        ));
        assert_eq!(
            workspace
                .resolve(
                    outside.path().join("secret.txt").to_str().unwrap(),
                    PathOperation::Read
                )
                .unwrap(),
            std::fs::canonicalize(outside.path().join("secret.txt")).unwrap()
        );
        assert!(matches!(
            workspace.resolve(
                outside.path().join("secret.txt").to_str().unwrap(),
                PathOperation::WriteExisting
            ),
            Err(WorkspaceError::OutsideRoots(_))
        ));
        assert!(matches!(
            workspace.resolve(
                outside.path().join("new.txt").to_str().unwrap(),
                PathOperation::Create
            ),
            Err(WorkspaceError::OutsideRoots(_))
        ));
        #[cfg(unix)]
        assert_eq!(
            workspace
                .resolve("link/secret.txt", PathOperation::Read)
                .unwrap(),
            std::fs::canonicalize(outside.path().join("secret.txt")).unwrap()
        );
    }

    #[test]
    fn creation_uses_nearest_existing_ancestor_and_additional_roots_are_stable() {
        let root = tempfile::tempdir().unwrap();
        let extra = tempfile::tempdir().unwrap();
        let primary = std::fs::canonicalize(root.path()).unwrap();
        let mut workspace = WorkspaceContext::from_primary(primary.clone(), primary);
        let created = workspace
            .resolve("new/deep/file.txt", PathOperation::Create)
            .unwrap();
        assert!(created.ends_with("new/deep/file.txt"));
        let first = workspace.add_root(extra.path(), true).unwrap();
        let second = workspace.add_root(extra.path(), true).unwrap();
        assert_eq!(first, second);
        assert!(matches!(
            workspace.resolve(
                extra.path().join("new.txt").to_str().unwrap(),
                PathOperation::Create
            ),
            Err(WorkspaceError::ReadOnly(_))
        ));
    }

    #[test]
    fn replacing_primary_preserves_supplemental_order_and_deduplicates() {
        let primary = tempfile::tempdir().unwrap();
        let replacement = tempfile::tempdir().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let mut workspace = WorkspaceContext::discover(primary.path()).unwrap();
        workspace.add_root(first.path(), false).unwrap();
        workspace.add_root(second.path(), true).unwrap();
        workspace.add_root(replacement.path(), false).unwrap();

        workspace.replace_primary(replacement.path()).unwrap();

        assert_eq!(workspace.cwd(), replacement.path().canonicalize().unwrap());
        assert_eq!(workspace.roots().len(), 3);
        assert!(workspace.roots()[0].primary);
        assert_eq!(
            workspace.roots()[0].path,
            replacement.path().canonicalize().unwrap()
        );
        assert_eq!(
            workspace.roots()[1].path,
            first.path().canonicalize().unwrap()
        );
        assert_eq!(
            workspace.roots()[2].path,
            second.path().canonicalize().unwrap()
        );
        assert!(workspace.roots()[2].read_only);
    }

    #[test]
    fn workspace_roots_must_be_directories() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("not-a-directory");
        std::fs::write(&file, "contents").unwrap();

        assert!(matches!(
            WorkspaceContext::discover(&file),
            Err(WorkspaceError::Missing(_))
        ));

        let mut workspace = WorkspaceContext::discover(temp.path()).unwrap();
        assert!(matches!(
            workspace.add_root(&file, false),
            Err(WorkspaceError::Missing(_))
        ));
        assert_eq!(workspace.roots().len(), 1);
    }

    #[test]
    fn skill_ancestor_roots_stop_at_primary_and_ignore_additional_roots() {
        let temp = tempfile::tempdir().unwrap();
        let primary = temp.path().join("repo");
        let cwd = primary.join("crates/api/src");
        let additional = temp.path().join("other");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&additional).unwrap();
        let mut workspace = WorkspaceContext::from_primary(
            std::fs::canonicalize(&primary).unwrap(),
            std::fs::canonicalize(&cwd).unwrap(),
        );
        workspace.add_root(&additional, false).unwrap();
        let cwd = std::fs::canonicalize(cwd).unwrap();
        let primary = std::fs::canonicalize(primary).unwrap();

        assert_eq!(
            workspace.skill_ancestor_roots(),
            vec![
                cwd.join(".agents/skills"),
                primary.join("crates/api/.agents/skills"),
                primary.join("crates/.agents/skills"),
                primary.join(".agents/skills"),
            ]
        );
        assert!(!workspace
            .skill_ancestor_roots()
            .iter()
            .any(|path| path.starts_with(&additional)));
    }

    #[test]
    fn preparation_normalizes_registry_resources_once_and_rejects_escape() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("file.txt"), "ok").unwrap();
        let primary = std::fs::canonicalize(root.path()).unwrap();
        let workspace = WorkspaceContext::from_primary(primary.clone(), primary.clone());
        let prepared = prepare_tool_invocation(
            &workspace,
            "read_file",
            &serde_json::json!({"path":"file.txt"}),
            crate::agent::registry::ToolOrigin::BuiltIn,
        )
        .unwrap();
        assert_eq!(
            prepared.arguments["path"],
            primary.join("file.txt").to_string_lossy().as_ref()
        );
        assert!(prepare_tool_invocation(
            &workspace,
            "write_file",
            &serde_json::json!({"path":"../outside.txt","content":"no"}),
            crate::agent::registry::ToolOrigin::BuiltIn,
        )
        .is_err());
        let command = prepare_tool_invocation(
            &workspace,
            "run_command",
            &serde_json::json!({"command":"true","network":true}),
            crate::agent::registry::ToolOrigin::BuiltIn,
        )
        .unwrap();
        assert_eq!(command.arguments["cwd"], primary.to_string_lossy().as_ref());
        assert!(command.network_requested);

        let error = prepare_tool_invocation(
            &workspace,
            "task",
            &serde_json::json!({"subagent_name":"invented-reviewer","prompt":"inspect"}),
            crate::agent::registry::ToolOrigin::BuiltIn,
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("unknown subagent: invented-reviewer"));
        assert!(message.contains("explore, plan, general"));
    }

    #[test]
    fn invalid_mechanistic_paths_are_not_reported_as_missing_workspace_paths() {
        let root = tempfile::tempdir().unwrap();
        let primary = std::fs::canonicalize(root.path()).unwrap();
        let workspace = WorkspaceContext::from_primary(primary.clone(), primary);
        let error = prepare_tool_invocation(
            &workspace,
            "delegate_mechanistic",
            &serde_json::json!({
                "actions": [{
                    "kind": "search_text",
                    "pattern": "needle",
                    "paths": ["/Users/example/repository/src"],
                    "max_matches": 8
                }]
            }),
            crate::agent::registry::ToolOrigin::BuiltIn,
        )
        .unwrap_err();
        assert!(matches!(error, WorkspaceError::InvalidArguments(_)));
        assert!(error
            .to_string()
            .contains("paths must be bounded relative paths"));
        assert!(!error.to_string().contains("workspace path does not exist"));
    }

    #[test]
    fn obvious_ssh_family_commands_request_network_without_model_hinting() {
        let root = tempfile::tempdir().unwrap();
        let primary = std::fs::canonicalize(root.path()).unwrap();
        let workspace = WorkspaceContext::from_primary(primary.clone(), primary);
        for command in [
            "ssh bsd true",
            "ssh -l aji bsd \"ps auxww | grep '[r]sync'\"",
            "/usr/bin/ssh -F /tmp/config bsd true",
            "scp local.txt bsd:/tmp/local.txt",
            "sftp bsd",
        ] {
            let prepared = prepare_tool_invocation(
                &workspace,
                "run_command",
                &serde_json::json!({"command":command}),
                crate::agent::registry::ToolOrigin::BuiltIn,
            )
            .unwrap();
            assert!(prepared.network_requested, "{command}");
        }
        for command in ["printf ssh", "rg ssh src", "./ssh bsd true"] {
            let prepared = prepare_tool_invocation(
                &workspace,
                "run_command",
                &serde_json::json!({"command":command}),
                crate::agent::registry::ToolOrigin::BuiltIn,
            )
            .unwrap();
            assert!(!prepared.network_requested, "{command}");
        }
    }

    #[test]
    fn git_network_subcommands_request_network_without_model_hinting() {
        let root = tempfile::tempdir().unwrap();
        let primary = std::fs::canonicalize(root.path()).unwrap();
        let workspace = WorkspaceContext::from_primary(primary.clone(), primary);
        for command in [
            "git push",
            "git fetch --all",
            "git pull --rebase",
            "git clone https://github.com/example/example.git",
            "git ls-remote https://github.com/example/example.git",
            "git -C /tmp/x push origin main",
            "git --no-pager push",
            "cd repo && git push",
            "git status && git push",
            "true; git fetch",
            "printf x | git pull",
            "/usr/bin/git push",
            "/opt/homebrew/bin/git fetch",
        ] {
            let prepared = prepare_tool_invocation(
                &workspace,
                "run_command",
                &serde_json::json!({"command":command}),
                crate::agent::registry::ToolOrigin::BuiltIn,
            )
            .unwrap();
            assert!(prepared.network_requested, "{command}");
        }
        for command in [
            "git status",
            "git diff",
            "git log --oneline",
            "git config --global user.name",
            "printf git push",
            "cd repo && git status",
            "git status && echo done",
        ] {
            let prepared = prepare_tool_invocation(
                &workspace,
                "run_command",
                &serde_json::json!({"command":command}),
                crate::agent::registry::ToolOrigin::BuiltIn,
            )
            .unwrap();
            assert!(!prepared.network_requested, "{command}");
        }
    }

    #[test]
    fn github_cli_and_http_clients_request_network_in_any_stage() {
        let root = tempfile::tempdir().unwrap();
        let primary = std::fs::canonicalize(root.path()).unwrap();
        let workspace = WorkspaceContext::from_primary(primary.clone(), primary);
        for command in [
            "gh pr list",
            "gh pr create --title t --body b",
            "gh api /user",
            "gh auth status",
            "/opt/homebrew/bin/gh repo view",
            "/usr/local/bin/gh issue list",
            "cd repo && gh pr create",
            "git push && gh pr create --fill",
            "gh auth status; git push",
            "curl -fsSL https://api.github.com/repos/example/example",
            "wget -qO- https://github.com",
            "rsync -a ./src host:/dst",
        ] {
            let prepared = prepare_tool_invocation(
                &workspace,
                "run_command",
                &serde_json::json!({"command":command}),
                crate::agent::registry::ToolOrigin::BuiltIn,
            )
            .unwrap();
            assert!(prepared.network_requested, "{command}");
        }
        for command in ["printf gh", "echo gh pr list", "./gh pr list"] {
            let prepared = prepare_tool_invocation(
                &workspace,
                "run_command",
                &serde_json::json!({"command":command}),
                crate::agent::registry::ToolOrigin::BuiltIn,
            )
            .unwrap();
            assert!(!prepared.network_requested, "{command}");
        }
    }

    #[test]
    fn remote_watchers_request_network_and_local_watchers_do_not() {
        let root = tempfile::tempdir().unwrap();
        let primary = std::fs::canonicalize(root.path()).unwrap();
        let workspace = WorkspaceContext::from_primary(primary.clone(), primary);
        let local = prepare_tool_invocation(
            &workspace,
            "watch_process",
            &serde_json::json!({
                "label":"local process",
                "host":{"type":"local"},
                "pid":42,
                "process":{"type":"process"},
                "interval_secs":10
            }),
            crate::agent::registry::ToolOrigin::BuiltIn,
        )
        .unwrap();
        assert!(!local.network_requested);

        let remote_arguments = serde_json::json!({
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
        let remote = prepare_tool_invocation(
            &workspace,
            "watch_process",
            &remote_arguments,
            crate::agent::registry::ToolOrigin::BuiltIn,
        )
        .unwrap();
        assert!(remote.network_requested);
        assert_eq!(remote.arguments, remote_arguments);
    }

    #[test]
    fn remote_mechanistic_query_and_apply_request_network() {
        let root = tempfile::tempdir().unwrap();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let query = prepare_tool_invocation(
            &workspace,
            "query_remote_rpc",
            &serde_json::json!({"endpoint":"seedbox","operation":"torrents"}),
            crate::agent::registry::ToolOrigin::BuiltIn,
        )
        .unwrap();
        assert!(query.network_requested);

        let apply = prepare_tool_invocation(
            &workspace,
            "apply_remote_sync",
            &serde_json::json!({
                "profile":"server_to_seedbox",
                "accepted_plan_digest":"a".repeat(64),
            }),
            crate::agent::registry::ToolOrigin::BuiltIn,
        )
        .unwrap();
        assert!(apply.network_requested);
    }

    #[test]
    fn workflow_offline_preview_is_local_but_probe_preview_and_run_request_network() {
        let root = tempfile::tempdir().unwrap();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let draft = serde_json::json!({
            "schema": "codex-team.workflow_draft.v1",
            "workflow_id": "seedbox_archive",
            "title": "Inspect completed torrents",
            "actions": [{
                "id": "query",
                "action": {
                    "kind": "query_transmission",
                    "endpoint": "seedbox",
                    "operation": "torrents"
                }
            }],
            "limits": {
                "max_runtime_seconds": 60,
                "min_free_space_bytes": 0,
                "deletion_allowed": false,
                "conflict_policy": "block",
                "verification": []
            },
            "rollback": {
                "kind": "unavailable",
                "limitation": "Read-only workflow has no mutation to roll back"
            }
        });
        for (name, arguments, expected) in [
            (
                "preview_automation_workflow",
                serde_json::json!({"draft":draft.clone(),"depth":"offline"}),
                false,
            ),
            (
                "preview_automation_workflow",
                serde_json::json!({"draft":draft.clone(),"depth":"read_only_probe"}),
                true,
            ),
            (
                "preview_automation_workflow",
                serde_json::json!({"draft":draft.clone(),"depth":"mutation_preview"}),
                true,
            ),
            (
                "run_automation_workflow",
                serde_json::json!({"workflow_id":"seedbox_archive"}),
                true,
            ),
        ] {
            let prepared = prepare_tool_invocation(
                &workspace,
                name,
                &arguments,
                crate::agent::registry::ToolOrigin::BuiltIn,
            )
            .unwrap();
            assert_eq!(prepared.network_requested, expected, "{name}");
        }
    }

    #[test]
    fn every_builtin_path_and_cwd_resource_is_prepared_or_rejected() {
        use crate::agent::registry::{ResourceKind, ToolHandler, ToolOrigin, ToolRegistry};
        use serde_json::json;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("file.txt"), "old").unwrap();
        let primary = std::fs::canonicalize(root.path()).unwrap();
        let outside_path = outside.path().join("escape.txt");
        std::fs::write(&outside_path, "outside").unwrap();
        let workspace = WorkspaceContext::from_primary(primary.clone(), primary.clone());
        let cases = vec![
            ("read_file", json!({"path":"file.txt"})),
            ("write_file", json!({"path":"new.txt", "content":"new"})),
            ("list_dir", json!({"path":"."})),
            ("run_command", json!({"command":"true"})),
            ("search_files", json!({"pattern":"old"})),
            (
                "edit_file",
                json!({"path":"file.txt", "old_string":"old", "new_string":"new"}),
            ),
            (
                "hashline_edit",
                json!({
                    "path":"file.txt",
                    "start_anchor":"00000000",
                    "end_anchor":"00000000",
                    "replacement":"new"
                }),
            ),
            ("apply_patch", json!({"path":"file.txt", "patch":"@@"})),
            ("delete_file", json!({"path":"file.txt"})),
            ("glob", json!({"pattern":"**/*.txt"})),
            ("read_lints", json!({})),
            ("read_todos", json!({})),
            ("update_todos", json!({"path":"file.txt", "action":"add"})),
            ("create_directory", json!({"path":"new-directory"})),
            (
                "fetch",
                json!({"url":"https://example.com", "dest":"download.txt"}),
            ),
            ("blame", json!({"path":"file.txt"})),
            (
                "generate_image",
                json!({"prompt":"image", "image_url":"file.txt"}),
            ),
        ];
        let registry = ToolRegistry::builtins();

        for (name, arguments) in cases {
            let spec = registry.get(name).unwrap();
            let prepared =
                prepare_tool_invocation(&workspace, name, &arguments, ToolOrigin::BuiltIn)
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
            for resource in &spec.resources {
                if !matches!(
                    resource.kind,
                    ResourceKind::Path | ResourceKind::WorkingDirectory
                ) {
                    continue;
                }
                let value = prepared.arguments[resource.field]
                    .as_str()
                    .unwrap_or_else(|| panic!("{name}.{} was not prepared", resource.field));
                assert!(Path::new(value).starts_with(&primary), "{name}: {value}");

                let mut escaped = arguments.clone();
                escaped.as_object_mut().unwrap().insert(
                    resource.field.into(),
                    Value::String(outside_path.to_string_lossy().into_owned()),
                );
                let operation_is_read = resource.kind == ResourceKind::WorkingDirectory
                    || !matches!(
                        spec.handler,
                        ToolHandler::WriteFile
                            | ToolHandler::CreateDirectory
                            | ToolHandler::Fetch
                            | ToolHandler::Browser
                            | ToolHandler::EditFile
                            | ToolHandler::HashlineEdit
                            | ToolHandler::ApplyPatch
                            | ToolHandler::DeleteFile
                            | ToolHandler::UpdateTodos
                    );
                let outside =
                    prepare_tool_invocation(&workspace, name, &escaped, ToolOrigin::BuiltIn);
                if operation_is_read {
                    assert_eq!(
                        outside.unwrap().arguments[resource.field],
                        std::fs::canonicalize(&outside_path)
                            .unwrap()
                            .to_string_lossy()
                            .as_ref(),
                        "{name}.{} did not preserve the canonical outside read",
                        resource.field
                    );
                } else {
                    assert!(
                        matches!(outside, Err(WorkspaceError::OutsideRoots(_))),
                        "{name}.{} accepted an outside write",
                        resource.field
                    );
                }
            }
        }

        let image = prepare_tool_invocation(
            &workspace,
            "generate_image",
            &json!({"prompt":"image"}),
            ToolOrigin::BuiltIn,
        )
        .unwrap();
        assert!(image.arguments.get("image_url").is_none());
    }

    #[test]
    fn browser_paths_are_confined_and_only_network_actions_request_network() {
        use crate::agent::registry::ToolOrigin;
        use serde_json::json;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let primary = std::fs::canonicalize(root.path()).unwrap();
        let workspace = WorkspaceContext::from_primary(primary.clone(), primary.clone());

        for action in ["start", "snapshot", "click", "type", "screenshot", "close"] {
            let arguments = if action == "screenshot" {
                json!({"action":action,"path":"shot.png"})
            } else {
                json!({"action":action})
            };
            let prepared =
                prepare_tool_invocation(&workspace, "browser", &arguments, ToolOrigin::BuiltIn)
                    .unwrap();
            assert!(!prepared.network_requested, "{action}");
            if action == "screenshot" {
                assert!(
                    Path::new(prepared.arguments["path"].as_str().unwrap()).starts_with(&primary)
                );
            }
        }
        for action in ["navigate", "download"] {
            let arguments = if action == "download" {
                json!({"action":action,"url":"https://example.com/file","path":"file.bin"})
            } else {
                json!({"action":action,"url":"https://example.com"})
            };
            assert!(
                prepare_tool_invocation(&workspace, "browser", &arguments, ToolOrigin::BuiltIn,)
                    .unwrap()
                    .network_requested
            );
        }
        assert!(matches!(
            prepare_tool_invocation(
                &workspace,
                "browser",
                &json!({
                    "action":"screenshot",
                    "path":outside.path().join("shot.png").to_string_lossy()
                }),
                ToolOrigin::BuiltIn,
            ),
            Err(WorkspaceError::OutsideRoots(_))
        ));
    }
}
