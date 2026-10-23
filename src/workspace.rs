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
    session_write_roots: Vec<SessionWriteRoot>,
}

/// A host-approved folder for this live session, excluded from saved roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionWriteRoot {
    pub path: PathBuf,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl SessionWriteRoot {
    pub fn prepare(path: &Path) -> Result<Self, WorkspaceError> {
        if !path.is_absolute() {
            return Err(WorkspaceError::InvalidArguments(
                "request_write_access requires an absolute folder path".into(),
            ));
        }
        let path = canonical_directory(path)?;
        let metadata = std::fs::metadata(&path)
            .map_err(|_| WorkspaceError::Missing(path.display().to_string()))?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            path,
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
        })
    }

    pub fn revalidate(&self) -> Result<(), WorkspaceError> {
        if Self::prepare(&self.path)? != *self {
            return Err(WorkspaceError::InvalidArguments(
                "Approved folder changed; request_write_access again".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum WorkspaceError {
    #[error("workspace path does not exist: {0}")]
    Missing(String),
    #[error("invalid tool arguments: {0}")]
    InvalidArguments(String),
    #[error("workspace path contains traversal: {0}")]
    Traversal(String),
    #[error("write path is outside approved folders: {0}; use request_write_access for the destination folder, then retry without sudo")]
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
            session_write_roots: Vec::new(),
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
            session_write_roots: Vec::new(),
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

    pub fn execution_roots(&self) -> Vec<PathBuf> {
        self.roots
            .iter()
            .map(|root| root.path.clone())
            .chain(
                self.session_write_roots
                    .iter()
                    .filter(|root| root.revalidate().is_ok())
                    .map(|root| root.path.clone()),
            )
            .collect()
    }

    pub fn grant_session_write(&mut self, root: SessionWriteRoot) -> Result<(), WorkspaceError> {
        root.revalidate()?;
        if self
            .roots
            .iter()
            .any(|existing| existing.read_only && root.path.starts_with(&existing.path))
        {
            return Err(WorkspaceError::ReadOnly(root.path.display().to_string()));
        }
        if !self.session_write_roots.contains(&root) {
            self.session_write_roots.push(root);
        }
        Ok(())
    }

    pub fn has_session_write(&self, path: &Path) -> bool {
        self.session_write_roots
            .iter()
            .any(|root| path.starts_with(&root.path) && root.revalidate().is_ok())
    }

    pub fn clear_session_writes(&mut self) {
        self.session_write_roots.clear();
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
            .find(|root| resolved.starts_with(&root.path));
        if let Some(root) = root {
            if root.read_only {
                return Err(WorkspaceError::ReadOnly(root.path.display().to_string()));
            }
        } else if !self.has_session_write(&resolved) {
            return Err(WorkspaceError::OutsideRoots(resolved.display().to_string()));
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
    let command_cwd = object
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.cwd().to_path_buf());
    let command_network = matches!(spec.handler, ToolHandler::RunCommand)
        && object
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| obvious_network_client_command(command, &command_cwd));
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

fn obvious_network_client_command(command: &str, cwd: &Path) -> bool {
    shell_command_stages(command)
        .into_iter()
        .any(|stage| stage_requests_network(&stage) || stage_script_requests_network(&stage, cwd))
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

const SCRIPT_SCAN_LIMIT: u64 = 256 * 1024;

#[derive(Clone, Copy)]
enum ScriptKind {
    Shell,
    Python,
}

/// A local program is not itself an obvious client, but a shell or Python
/// wrapper often execs `ssh` (or curl, git push, and the rest). Classify the
/// script the command will run so that wrapper gets the same network grant as
/// typing the client directly.
fn stage_script_requests_network(stage: &str, cwd: &Path) -> bool {
    let words = shell_words(stage);
    let Some(program) = words.first() else {
        return false;
    };
    if !plain_program_token(program) {
        return false;
    }
    let base = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    if program == "." || program == "source" {
        return words.get(1).is_some_and(|path| {
            resolved_script_requests_network(path, cwd, Some(ScriptKind::Shell))
        });
    }
    if is_shell_interpreter(base) {
        if let Some(script) = inline_c_argument(&words) {
            return shell_text_requests_network(script);
        }
        if let Some(path) = interpreter_file_argument(&words) {
            return resolved_script_requests_network(path, cwd, Some(ScriptKind::Shell));
        }
        return false;
    }
    if is_python_interpreter(base) {
        if let Some(script) = inline_c_argument(&words) {
            return python_text_requests_network(script);
        }
        if let Some(path) = interpreter_file_argument(&words) {
            return resolved_script_requests_network(path, cwd, Some(ScriptKind::Python));
        }
        return false;
    }
    resolved_script_requests_network(program, cwd, None)
}

fn plain_program_token(program: &str) -> bool {
    !program.is_empty()
        && !program.bytes().any(|byte| {
            matches!(
                byte,
                b'\'' | b'"' | b'\\' | b'$' | b'`' | b';' | b'|' | b'&' | b'\n' | b'\r'
            )
        })
}

fn is_shell_interpreter(name: &str) -> bool {
    matches!(name, "sh" | "bash" | "zsh" | "ksh" | "dash" | "ash")
}

fn is_python_interpreter(name: &str) -> bool {
    name == "python"
        || name == "python3"
        || name
            .strip_prefix("python3.")
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()))
}

fn inline_c_argument(words: &[String]) -> Option<&str> {
    let mut index = 1;
    while index < words.len() {
        let word = &words[index];
        if word == "--" {
            return None;
        }
        if word == "-c" || (word.starts_with('-') && !word.starts_with("--") && word.contains('c'))
        {
            return words.get(index + 1).map(String::as_str);
        }
        if word.starts_with('-') {
            index += 1;
            continue;
        }
        return None;
    }
    None
}

fn interpreter_file_argument(words: &[String]) -> Option<&str> {
    let mut index = 1;
    while index < words.len() {
        let word = &words[index];
        if word == "--" {
            return words.get(index + 1).map(String::as_str);
        }
        if word == "-c" || (word.starts_with('-') && !word.starts_with("--") && word.contains('c'))
        {
            return None;
        }
        if word.starts_with('-') {
            index += 1;
            continue;
        }
        return Some(word.as_str());
    }
    None
}

fn resolved_script_requests_network(program: &str, cwd: &Path, kind: Option<ScriptKind>) -> bool {
    if !plain_program_token(program) {
        return false;
    }
    let Some(path) = resolve_program_file(program, cwd) else {
        return false;
    };
    file_requests_network(&path, kind)
}

fn resolve_program_file(program: &str, cwd: &Path) -> Option<PathBuf> {
    let expanded = if let Some(rest) = program.strip_prefix("~/") {
        PathBuf::from(std::env::var_os("HOME")?).join(rest)
    } else {
        PathBuf::from(program)
    };
    if expanded.is_absolute() {
        return expanded.is_file().then_some(expanded);
    }
    if program.contains('/') {
        let candidate = cwd.join(expanded);
        return candidate.is_file().then_some(candidate);
    }
    let paths = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&paths) {
        let candidate = directory.join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn file_requests_network(path: &Path, forced: Option<ScriptKind>) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > SCRIPT_SCAN_LIMIT {
        return false;
    }
    let Some(kind) = forced.or_else(|| detect_script_kind(path)) else {
        return false;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    if text.contains('\0') || text.len() > SCRIPT_SCAN_LIMIT as usize {
        return false;
    }
    match kind {
        ScriptKind::Shell => shell_text_requests_network(&text),
        ScriptKind::Python => python_text_requests_network(&text),
    }
}

fn detect_script_kind(path: &Path) -> Option<ScriptKind> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("py") => return Some(ScriptKind::Python),
        Some("sh" | "bash" | "zsh" | "ksh") => return Some(ScriptKind::Shell),
        _ => {}
    }
    let mut header = [0u8; 128];
    let count = std::fs::File::open(path)
        .and_then(|mut file| std::io::Read::read(&mut file, &mut header))
        .ok()?;
    if header[..count].contains(&0) {
        return None;
    }
    let header = String::from_utf8_lossy(&header[..count]);
    let line = header.lines().next().unwrap_or("");
    let rest = line.strip_prefix("#!")?.trim();
    let rest = rest
        .strip_prefix("/usr/bin/env -S ")
        .or_else(|| rest.strip_prefix("/usr/bin/env "))
        .unwrap_or(rest);
    let interpreter = rest.split_whitespace().next().unwrap_or("");
    let base = Path::new(interpreter)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(interpreter);
    if is_python_interpreter(base) {
        Some(ScriptKind::Python)
    } else if is_shell_interpreter(base) {
        Some(ScriptKind::Shell)
    } else {
        None
    }
}

fn shell_text_requests_network(text: &str) -> bool {
    shell_text_requests_network_depth(text, 0)
}

fn shell_text_requests_network_depth(text: &str, depth: usize) -> bool {
    if depth > 8 || text.len() > SCRIPT_SCAN_LIMIT as usize {
        return false;
    }
    let cleaned = strip_shell_comments(text);
    if command_substitutions(&cleaned)
        .into_iter()
        .any(|body| shell_text_requests_network_depth(&body, depth + 1))
    {
        return true;
    }
    shell_commands_request_network(&cleaned)
}

fn python_text_requests_network(text: &str) -> bool {
    if text.len() > SCRIPT_SCAN_LIMIT as usize {
        return false;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '#' => {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
            }
            '"' | '\'' => index = skip_python_string(&chars, index),
            '[' => {
                if python_list_requests_network(&chars, index + 1) {
                    return true;
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    false
}

fn skip_python_string(chars: &[char], start: usize) -> usize {
    let quote = chars[start];
    let triple = start + 2 < chars.len() && chars[start + 1] == quote && chars[start + 2] == quote;
    let mut index = if triple { start + 3 } else { start + 1 };
    while index < chars.len() {
        if chars[index] == '\\' {
            index = index.saturating_add(2);
            continue;
        }
        if triple
            && index + 2 < chars.len()
            && chars[index] == quote
            && chars[index + 1] == quote
            && chars[index + 2] == quote
        {
            return index + 3;
        }
        if !triple && chars[index] == quote {
            return index + 1;
        }
        if !triple && chars[index] == '\n' {
            return index;
        }
        index += 1;
    }
    index
}

fn python_list_requests_network(chars: &[char], mut index: usize) -> bool {
    let Some(program) = next_python_string(chars, &mut index) else {
        return false;
    };
    let mut arguments = Vec::new();
    loop {
        index = skip_python_whitespace(chars, index);
        if index >= chars.len() || chars[index] == ']' {
            break;
        }
        if chars[index] == ',' {
            index += 1;
            continue;
        }
        let Some(value) = next_python_string(chars, &mut index) else {
            break;
        };
        arguments.push(value);
    }
    shell_command_word_requests_network(&program, &arguments)
}

fn next_python_string(chars: &[char], index: &mut usize) -> Option<String> {
    *index = skip_python_whitespace(chars, *index);
    if *index >= chars.len() || (chars[*index] != '"' && chars[*index] != '\'') {
        return None;
    }
    let quote = chars[*index];
    if *index + 2 < chars.len() && chars[*index + 1] == quote && chars[*index + 2] == quote {
        return None;
    }
    let start = *index;
    *index = skip_python_string(chars, *index);
    if *index <= start + 1 || chars[start] != quote {
        return None;
    }
    Some(chars[start + 1..*index - 1].iter().collect())
}

fn skip_python_whitespace(chars: &[char], mut index: usize) -> usize {
    while index < chars.len() && chars[index].is_whitespace() {
        index += 1;
    }
    index
}

fn shell_command_word_requests_network(program: &str, arguments: &[String]) -> bool {
    if !plain_program_token(program) || matches!(program, "$(...)" | "`...`" | "${...}") {
        return false;
    }
    let mut stage = String::from(program);
    for argument in arguments {
        if argument.bytes().any(|byte| byte.is_ascii_whitespace()) {
            break;
        }
        stage.push(' ');
        stage.push_str(argument);
    }
    stage_requests_network(&stage)
}

fn strip_shell_comments(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut index = 0;
    let mut quote: Option<char> = None;
    while index < chars.len() {
        let ch = chars[index];
        if let Some(active) = quote {
            out.push(ch);
            if ch == '\\' && active == '"' && index + 1 < chars.len() {
                out.push(chars[index + 1]);
                index += 2;
                continue;
            }
            if ch == active {
                quote = None;
            }
            index += 1;
            continue;
        }
        if ch == '\\'
            && index + 1 < chars.len()
            && (chars[index + 1] == '\n' || chars[index + 1] == '\r')
        {
            index += 1;
            if chars[index] == '\r' {
                index += 1;
            }
            if index < chars.len() && chars[index] == '\n' {
                index += 1;
            }
            continue;
        }
        if ch == '\r' {
            index += 1;
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            out.push(ch);
            index += 1;
            continue;
        }
        if ch == '#' && shell_comment_starts(&out) {
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }
        out.push(ch);
        index += 1;
    }
    out
}

fn shell_comment_starts(text: &str) -> bool {
    match text.chars().next_back() {
        None => true,
        Some(ch) => ch.is_whitespace() || matches!(ch, ';' | '|' | '&' | '(' | '{' | ')'),
    }
}

fn command_substitutions(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut bodies = Vec::new();
    let mut index = 0;
    let mut quote: Option<char> = None;
    while index < chars.len() {
        let ch = chars[index];
        if let Some(active) = quote {
            if ch == '\\' && active == '"' && index + 1 < chars.len() {
                index += 2;
                continue;
            }
            if ch == active {
                quote = None;
            }
            index += 1;
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            index += 1;
            continue;
        }
        if ch == '`' {
            let start = index + 1;
            index += 1;
            while index < chars.len() && chars[index] != '`' {
                index += 1;
            }
            if index < chars.len() {
                bodies.push(chars[start..index].iter().collect());
                index += 1;
            }
            continue;
        }
        if ch == '$' && index + 1 < chars.len() && chars[index + 1] == '(' {
            index += 2;
            let start = index;
            let mut depth = 1;
            let mut inner_quote: Option<char> = None;
            while index < chars.len() && depth > 0 {
                let inner = chars[index];
                if let Some(active) = inner_quote {
                    if inner == '\\' && active == '"' && index + 1 < chars.len() {
                        index += 2;
                        continue;
                    }
                    if inner == active {
                        inner_quote = None;
                    }
                    index += 1;
                    continue;
                }
                match inner {
                    '\'' | '"' => inner_quote = Some(inner),
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                index += 1;
            }
            if depth == 0 {
                bodies.push(chars[start..index - 1].iter().collect());
            }
            continue;
        }
        index += 1;
    }
    bodies
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShellScan {
    Command,
    Args,
    Prefix,
    ForList,
    CaseScrutinee,
    CasePattern,
    CaseBody,
}

fn shell_commands_request_network(text: &str) -> bool {
    let tokens = shell_tokens(text);
    let mut index = 0;
    let mut mode = ShellScan::Command;
    while index < tokens.len() {
        if mode == ShellScan::Prefix {
            match &tokens[index] {
                ShellToken::Word(word) if word == "--" => {
                    mode = ShellScan::Command;
                    index += 1;
                }
                ShellToken::Word(word) if word.starts_with('-') || is_shell_assignment(word) => {
                    index += 1;
                }
                _ => mode = ShellScan::Command,
            }
            continue;
        }
        match mode {
            ShellScan::ForList => {
                if matches!(&tokens[index], ShellToken::Word(word) if word == "do")
                    || matches!(tokens[index], ShellToken::OpenBrace)
                {
                    mode = ShellScan::Command;
                }
                index += 1;
                continue;
            }
            ShellScan::CaseScrutinee => {
                if let ShellToken::Word(word) = &tokens[index] {
                    if word == "in" {
                        mode = ShellScan::CasePattern;
                    } else if word == "esac" {
                        mode = ShellScan::Command;
                    }
                }
                index += 1;
                continue;
            }
            ShellScan::CasePattern => {
                match &tokens[index] {
                    ShellToken::Word(word) if word == "esac" => mode = ShellScan::Command,
                    ShellToken::CloseParen => mode = ShellScan::CaseBody,
                    ShellToken::Word(word) if word.ends_with(')') => mode = ShellScan::CaseBody,
                    _ => {}
                }
                index += 1;
                continue;
            }
            ShellScan::Prefix => unreachable!("prefix is handled above"),
            ShellScan::Command | ShellScan::Args | ShellScan::CaseBody => {}
        }
        let in_case = mode == ShellScan::CaseBody;
        match &tokens[index] {
            ShellToken::CaseSep if in_case => mode = ShellScan::CasePattern,
            ShellToken::Newline
            | ShellToken::Semi
            | ShellToken::And
            | ShellToken::Or
            | ShellToken::Pipe
            | ShellToken::Amp
            | ShellToken::CaseSep => {
                mode = if in_case {
                    ShellScan::CaseBody
                } else {
                    ShellScan::Command
                };
            }
            ShellToken::OpenBrace
            | ShellToken::OpenParen
            | ShellToken::CloseBrace
            | ShellToken::CloseParen => {
                mode = if in_case {
                    ShellScan::CaseBody
                } else {
                    ShellScan::Command
                };
            }
            ShellToken::Redirect => {
                if matches!(tokens.get(index + 1), Some(ShellToken::Word(_))) {
                    index += 1;
                }
            }
            ShellToken::Word(word) if mode == ShellScan::Args => {
                if word == "esac" && in_case {
                    mode = ShellScan::Command;
                }
            }
            ShellToken::Word(word) => {
                if word == "esac" && in_case {
                    mode = ShellScan::Command;
                } else if word == "for" || word == "select" {
                    mode = ShellScan::ForList;
                    index += 1;
                    if let Some(ShellToken::Word(next)) = tokens.get(index) {
                        if next != "in" && next != "do" {
                            index += 1;
                        }
                    }
                    continue;
                } else if word == "case" {
                    mode = ShellScan::CaseScrutinee;
                } else if is_shell_keyword(word) || is_shell_assignment(word) {
                    mode = if in_case {
                        ShellScan::CaseBody
                    } else {
                        ShellScan::Command
                    };
                } else if is_command_prefix(word) {
                    mode = ShellScan::Prefix;
                } else {
                    let arguments = following_shell_words(&tokens, index + 1);
                    if shell_command_word_requests_network(word, &arguments) {
                        return true;
                    }
                    mode = ShellScan::Args;
                }
            }
        }
        index += 1;
    }
    false
}

fn is_shell_keyword(word: &str) -> bool {
    matches!(
        word,
        "if" | "elif"
            | "then"
            | "else"
            | "fi"
            | "while"
            | "until"
            | "do"
            | "done"
            | "in"
            | "coproc"
            | "!"
            | "{"
            | "}"
            | "time"
    )
}

fn is_command_prefix(word: &str) -> bool {
    matches!(
        word,
        "exec" | "command" | "builtin" | "sudo" | "env" | "nice" | "nohup" | "stdbuf"
    )
}

fn is_shell_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    match chars.next() {
        Some(ch) if ch == '_' || ch.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn following_shell_words(tokens: &[ShellToken], start: usize) -> Vec<String> {
    let mut words = Vec::new();
    for token in tokens.iter().skip(start) {
        match token {
            ShellToken::Word(word) => words.push(word.clone()),
            _ => break,
        }
    }
    words
}

enum ShellToken {
    Word(String),
    Newline,
    Semi,
    And,
    Or,
    Pipe,
    Amp,
    CaseSep,
    OpenParen,
    CloseParen,
    OpenBrace,
    CloseBrace,
    Redirect,
}

fn shell_words(text: &str) -> Vec<String> {
    shell_tokens(text)
        .into_iter()
        .filter_map(|token| match token {
            ShellToken::Word(word) => Some(word),
            _ => None,
        })
        .collect()
}

fn shell_tokens(text: &str) -> Vec<ShellToken> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_whitespace() && ch != '\n' {
            index += 1;
            continue;
        }
        if ch == '\n' {
            tokens.push(ShellToken::Newline);
            index += 1;
            continue;
        }
        if ch == ';' && index + 1 < chars.len() && chars[index + 1] == ';' {
            tokens.push(ShellToken::CaseSep);
            index += 2;
            continue;
        }
        if let Some((token, next)) = shell_operator(&chars, index) {
            tokens.push(token);
            index = next;
            continue;
        }
        let (word, next) = shell_word(&chars, index);
        index = next;
        if !word.is_empty() {
            tokens.push(ShellToken::Word(word));
        }
    }
    tokens
}

fn shell_operator(chars: &[char], index: usize) -> Option<(ShellToken, usize)> {
    let ch = chars[index];
    let next = chars.get(index + 1).copied();
    match (ch, next) {
        ('&', Some('&')) => Some((ShellToken::And, index + 2)),
        ('|', Some('|')) => Some((ShellToken::Or, index + 2)),
        ('|', Some('&')) => Some((ShellToken::Pipe, index + 2)),
        (';', _) => Some((ShellToken::Semi, index + 1)),
        ('&', _) => Some((ShellToken::Amp, index + 1)),
        ('|', _) => Some((ShellToken::Pipe, index + 1)),
        ('(', _) => Some((ShellToken::OpenParen, index + 1)),
        (')', _) => Some((ShellToken::CloseParen, index + 1)),
        ('{', _) => Some((ShellToken::OpenBrace, index + 1)),
        ('}', _) => Some((ShellToken::CloseBrace, index + 1)),
        ('<', Some('<')) => Some((
            ShellToken::Redirect,
            if chars.get(index + 2) == Some(&'<') {
                index + 3
            } else {
                index + 2
            },
        )),
        ('<', Some('>')) | ('<', Some('&')) | ('>', Some('&')) => {
            Some((ShellToken::Redirect, index + 2))
        }
        ('>', Some('>')) => Some((ShellToken::Redirect, index + 2)),
        ('<' | '>', _) => Some((ShellToken::Redirect, index + 1)),
        _ => None,
    }
}

fn shell_word(chars: &[char], mut index: usize) -> (String, usize) {
    let mut word = String::new();
    let mut quote: Option<char> = None;
    while index < chars.len() {
        let ch = chars[index];
        if let Some(active) = quote {
            if ch == '\\' && active == '"' && index + 1 < chars.len() {
                word.push(chars[index + 1]);
                index += 2;
                continue;
            }
            if ch == active {
                quote = None;
                index += 1;
                continue;
            }
            word.push(ch);
            index += 1;
            continue;
        }
        if ch == '\\' && index + 1 < chars.len() {
            word.push(chars[index + 1]);
            index += 2;
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            index += 1;
            continue;
        }
        if ch == '$' && index + 1 < chars.len() && chars[index + 1] == '(' {
            index = skip_balanced_paren(chars, index + 2);
            if word.is_empty() {
                word.push_str("$(...)");
            }
            continue;
        }
        if ch == '$' && index + 1 < chars.len() && chars[index + 1] == '{' {
            index = skip_balanced_brace(chars, index + 2);
            if word.is_empty() {
                word.push_str("${...}");
            }
            continue;
        }
        if ch == '`' {
            index += 1;
            while index < chars.len() && chars[index] != '`' {
                index += 1;
            }
            if index < chars.len() {
                index += 1;
            }
            if word.is_empty() {
                word.push_str("`...`");
            }
            continue;
        }
        if ch.is_whitespace() || shell_operator(chars, index).is_some() {
            break;
        }
        word.push(ch);
        index += 1;
    }
    (word, index)
}

fn skip_balanced_brace(chars: &[char], mut index: usize) -> usize {
    let mut depth = 1;
    while index < chars.len() && depth > 0 {
        match chars[index] {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        index += 1;
    }
    index
}

fn skip_balanced_paren(chars: &[char], mut index: usize) -> usize {
    let mut depth = 1;
    let mut quote: Option<char> = None;
    while index < chars.len() && depth > 0 {
        let ch = chars[index];
        if let Some(active) = quote {
            if ch == '\\' && active == '"' && index + 1 < chars.len() {
                index += 2;
                continue;
            }
            if ch == active {
                quote = None;
            }
            index += 1;
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        index += 1;
    }
    index
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

    fn command_requests_network(cwd: &Path, command: &str) -> bool {
        let workspace = WorkspaceContext::from_primary(cwd.to_path_buf(), cwd.to_path_buf());
        prepare_tool_invocation(
            &workspace,
            "run_command",
            &serde_json::json!({"command": command}),
            crate::agent::registry::ToolOrigin::BuiltIn,
        )
        .unwrap()
        .network_requested
    }

    #[test]
    fn shell_wrapper_network_clients_are_classified_from_script_text() {
        for script in [
            "elif ssh -n -o BatchMode=yes \\\n    root@bsdsan /bin/true; then\n    true\nfi\n",
            "exec ssh -o BatchMode=yes root@example true\n",
            "if ! ssh root@example true; then true; fi\n",
            "ssh_query() {\n  ssh -n root@example \"$@\"\n}\n",
            "case \"$mode\" in\n  live) ssh root@example true ;;\nesac\n",
            "git push origin main\n",
            "cd repo && curl -fsSL https://example.test\n",
        ] {
            assert!(shell_text_requests_network(script), "{script}");
        }
        for script in [
            "# ssh root@example\necho ssh\nprintf 'ssh %s\\n' x\n",
            "for ssh in a b; do echo \"$ssh\"; done\n",
            "git status\n",
            "git diff && git log --oneline\n",
            "message='ssh: connect to host bsdsan port 22'\nprintf '%s\\n' \"$message\"\n",
        ] {
            assert!(!shell_text_requests_network(script), "{script}");
        }
    }

    #[test]
    fn python_wrapper_network_clients_are_classified_from_script_text() {
        for script in [
            "SSH = [\"ssh\", \"-n\", \"-o\", \"BatchMode=yes\"]\n",
            "subprocess.run([\"/usr/bin/ssh\", \"-n\", host])\n",
            "subprocess.Popen([\n    \"scp\",\n    \"local.txt\",\n    \"host:remote.txt\",\n])\n",
            "[\"git\", \"push\"]\n",
            "[\"/opt/homebrew/bin/gh\", \"pr\", \"list\"]\n",
        ] {
            assert!(python_text_requests_network(script), "{script}");
        }
        for script in [
            "raise SystemExit(\"ssh not found\")\n",
            "\"\"\"Needs the same ssh access as the status tool.\"\"\"\nprint('ok')\n",
            "# [\"ssh\", \"host\"]\nprint('offline')\n",
            "[\"git\", \"status\"]\n",
            "[\"/tmp/ssh\", \"host\"]\n",
            "ssh = True\n",
        ] {
            assert!(!python_text_requests_network(script), "{script}");
        }
    }

    #[test]
    fn local_wrapper_network_paths_follow_the_script_they_execute() {
        let root = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(root.path()).unwrap();
        let shell = root.join("bsdsan-dedupe-status");
        std::fs::write(
            &shell,
            "#!/bin/bash\nif false; then\n:\nelif ssh -n -o BatchMode=yes \\\n    root@bsdsan /bin/true; then\n:\nfi\n",
        )
        .unwrap();
        assert!(command_requests_network(
            &root,
            &format!("{} --once --details", shell.display())
        ));
        assert!(command_requests_network(
            &root,
            "./bsdsan-dedupe-status --once"
        ));
        assert!(command_requests_network(
            &root,
            "bash -c 'exec ssh root@example true'"
        ));
        assert!(!command_requests_network(&root, "bash -c 'echo ssh'"));

        let sourced = root.join("inner.sh");
        std::fs::write(&sourced, "#!/bin/sh\nexec ssh root@example true\n").unwrap();
        assert!(command_requests_network(
            &root,
            &format!("bash {}", sourced.display())
        ));

        let python = root.join("status.py");
        std::fs::write(
            &python,
            "#!/usr/bin/env python3\nSSH = [\"ssh\", \"-n\", \"-o\", \"BatchMode=yes\"]\n",
        )
        .unwrap();
        assert!(command_requests_network(
            &root,
            &python.display().to_string()
        ));
        assert!(command_requests_network(
            &root,
            &format!("python3 {}", python.display())
        ));
        assert!(command_requests_network(
            &root,
            r#"python3 -c 'subprocess.run(["ssh", "host"])'"#
        ));
        assert!(!command_requests_network(
            &root,
            r#"python3 -c 'raise SystemExit("ssh not found")'"#
        ));

        let mention = root.join("mention.sh");
        std::fs::write(
            &mention,
            "#!/bin/bash\necho 'ssh: connect to host bsdsan port 22: Operation not permitted'\n",
        )
        .unwrap();
        assert!(!command_requests_network(
            &root,
            &mention.display().to_string()
        ));

        let fake_ssh = root.join("ssh");
        std::fs::write(&fake_ssh, "#!/bin/bash\necho offline\n").unwrap();
        assert!(!command_requests_network(&root, "./ssh bsd true"));
        std::fs::write(&fake_ssh, "#!/bin/bash\n/usr/bin/ssh bsd true\n").unwrap();
        assert!(command_requests_network(&root, "./ssh bsd true"));
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
