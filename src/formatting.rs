//! Opt-in, read-only formatting of candidate bytes before an approved native edit.
use crate::undo::FileState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, RwLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TOKEN: &str = "_ca_edit_token";
const PREVIEW: &str = "_ca_edit_preview";
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FormatterConfig {
    pub enabled: bool,
    pub presets: Vec<String>,
    pub custom: Vec<FormatterRule>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormatterRule {
    pub extensions: Vec<String>,
    pub command: Vec<String>,
}
#[derive(Clone, Default)]
struct Runtime {
    config: FormatterConfig,
}
fn runtime() -> &'static RwLock<Runtime> {
    static R: OnceLock<RwLock<Runtime>> = OnceLock::new();
    R.get_or_init(|| RwLock::new(Runtime::default()))
}
impl FormatterConfig {
    pub fn is_default(&self) -> bool {
        !self.enabled && self.presets.is_empty() && self.custom.is_empty()
    }
}
pub fn validate(config: &FormatterConfig) -> Result<(), String> {
    for preset in &config.presets {
        if !matches!(preset.as_str(), "rustfmt" | "prettier" | "ruff") {
            return Err(format!("Unknown formatter preset: {preset}"));
        }
    }
    for rule in &config.custom {
        if rule.command.is_empty()
            || rule.command.iter().any(|s| s.contains('\0'))
            || rule.extensions.is_empty()
        {
            return Err("Formatter rules require extensions and a nonempty argv command".into());
        }
    }
    Ok(())
}

pub fn install(config: &crate::config::CaConfig) -> Result<(), String> {
    validate(&config.formatting)?;
    *runtime().write().unwrap_or_else(|p| p.into_inner()) = Runtime {
        config: config.formatting.clone(),
    };
    Ok(())
}
#[derive(Clone)]
struct FrozenEdit {
    name: String,
    arguments: Value,
    path: PathBuf,
    before: FileState,
    after: FileState,
    warning: Option<String>,
}
#[derive(Default)]
struct Tickets {
    entries: HashMap<String, FrozenEdit>,
    order: VecDeque<String>,
}
fn tickets() -> &'static Mutex<Tickets> {
    static T: OnceLock<Mutex<Tickets>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(Tickets::default()))
}
pub fn plain_arguments(args: &Value) -> Value {
    let mut args = args.clone();
    if let Some(object) = args.as_object_mut() {
        object.remove(TOKEN);
        object.remove(PREVIEW);
    }
    args
}
pub fn has_ticket(args: &Value) -> bool {
    args.get(TOKEN).is_some()
}
pub fn consume(
    name: &str,
    args: &Value,
) -> Result<(crate::undo::PreparedFileMutation, String), String> {
    let token = args
        .get(TOKEN)
        .and_then(Value::as_str)
        .ok_or("Formatted edit lacks its host ticket")?;
    let mut store = tickets().lock().unwrap_or_else(|p| p.into_inner());
    let frozen = store
        .entries
        .remove(token)
        .ok_or("Formatted edit expired; prepare and approve the edit again")?;
    store.order.retain(|id| id != token);
    if frozen.name != name || frozen.arguments != plain_arguments(args) {
        return Err("Formatted edit arguments changed after preparation".into());
    }
    let marker = frozen.warning.map_or_else(
        || {
            format!(
                "Edited {} (formatted before publication).",
                frozen.path.display()
            )
        },
        |warning| {
            format!(
                "Edited {}. [Formatter warning: {warning}]",
                frozen.path.display()
            )
        },
    );
    Ok((
        crate::undo::PreparedFileMutation::single(frozen.path, name, frozen.before, frozen.after)?,
        marker,
    ))
}
fn select(config: &FormatterConfig, path: &Path) -> Option<Vec<String>> {
    let extension = path.extension()?.to_str()?;
    if let Some(rule) = config.custom.iter().find(|r| {
        r.extensions
            .iter()
            .any(|e| e.trim_start_matches('.') == extension)
    }) {
        return Some(rule.command.clone());
    }
    for preset in &config.presets {
        match preset.as_str() {
            "rustfmt" if extension == "rs" => {
                return Some(vec!["rustfmt".into(), "--emit".into(), "stdout".into()])
            }
            "ruff" if matches!(extension, "py" | "pyi") => {
                return Some(vec![
                    "ruff".into(),
                    "format".into(),
                    "--stdin-filename".into(),
                    "$FILE".into(),
                    "-".into(),
                ])
            }
            "prettier"
                if matches!(
                    extension,
                    "js" | "jsx"
                        | "ts"
                        | "tsx"
                        | "json"
                        | "css"
                        | "scss"
                        | "html"
                        | "md"
                        | "yaml"
                        | "yml"
                ) =>
            {
                return Some(vec![
                    "prettier".into(),
                    "--stdin-filepath".into(),
                    "$FILE".into(),
                ])
            }
            _ => {}
        }
    }
    None
}
async fn propose(name: &str, args: &Value, before: &FileState) -> Result<String, String> {
    let source = match before {
        FileState::Utf8(text) => text.as_str(),
        FileState::Missing => "",
    };
    match name {
        "write_file" => Ok(args["content"].as_str().unwrap_or_default().into()),
        "edit_file" => {
            let old = args["old_string"].as_str().unwrap_or_default();
            if old.is_empty() || source.matches(old).count() != 1 {
                return Err("Edit source must match exactly once".into());
            }
            Ok(source.replacen(old, args["new_string"].as_str().unwrap_or_default(), 1))
        }
        "apply_patch" => crate::agent::tools::apply_unified_patch(
            source,
            args["patch"].as_str().unwrap_or_default(),
        ),
        "hashline_edit" => crate::agent::tools::apply_hashline_edit(
            source,
            args["start_anchor"].as_str().unwrap_or_default(),
            args["end_anchor"].as_str().unwrap_or_default(),
            args["replacement"].as_str().unwrap_or_default(),
        )
        .map(|v| v.content),
        _ => Err("Not a native file edit".into()),
    }
}

/// Called only after original provider arguments passed the strict tool schema and path policy.
pub async fn prepare_with_authority(
    name: &str,
    args: &mut Value,
    workspace: &crate::workspace::WorkspaceContext,
    sandbox: crate::sandbox::SandboxMode,
    authorize: impl Fn(&Value) -> bool,
) -> Result<(), String> {
    if !matches!(
        name,
        "write_file" | "edit_file" | "apply_patch" | "hashline_edit"
    ) {
        return Ok(());
    }
    let settings = runtime().read().unwrap_or_else(|p| p.into_inner()).clone();
    if !settings.config.enabled {
        return Ok(());
    }
    let path = PathBuf::from(args["path"].as_str().ok_or("File edit has no path")?);
    let Some(mut argv) = select(&settings.config, &path) else {
        return Ok(());
    };
    let target = path.clone();
    let before = tokio::task::spawn_blocking(move || crate::undo::read_file_state(&target))
        .await
        .map_err(|_| "Formatter source worker stopped")??;
    let candidate = propose(name, args, &before).await?;
    FileState::utf8(candidate.clone())?;
    for arg in &mut argv {
        *arg = arg.replace("$FILE", &path.to_string_lossy());
    }
    if argv.first().is_some_and(|name| name == "rustfmt") {
        for directory in path.parent().into_iter().flat_map(Path::ancestors) {
            let manifest = directory.join("Cargo.toml");
            if let Ok(file) = tokio::fs::File::open(manifest).await {
                let mut bytes = Vec::new();
                if file.take(128 * 1024).read_to_end(&mut bytes).await.is_ok() {
                    if let Ok(document) = std::str::from_utf8(&bytes)
                        .unwrap_or("")
                        .parse::<toml::Value>()
                    {
                        let edition = document
                            .get("package")
                            .and_then(|p| p.get("edition"))
                            .and_then(toml::Value::as_str)
                            .or_else(|| {
                                document
                                    .get("workspace")
                                    .and_then(|w| w.get("package"))
                                    .and_then(|p| p.get("edition"))
                                    .and_then(toml::Value::as_str)
                            });
                        if let Some(edition) = edition {
                            argv.extend(["--edition".into(), edition.into()]);
                            break;
                        }
                    }
                }
            }
        }
    }
    let invocation = serde_json::json!({"command": argv.iter().map(|a| format!("'{}'", a.replace('\'', "'\"'\"'"))).collect::<Vec<_>>().join(" "), "cwd": workspace.cwd(), "network": false});
    let permitted = authorize(&invocation);
    let formatted = if permitted {
        run(&argv, &candidate, &path, workspace, sandbox).await
    } else {
        Err("formatter requires an explicit allowed command rule; requested edit kept".into())
    };
    let (after, warning) = match formatted {
        Ok(text) => (FileState::utf8(text)?, None),
        Err(warning) => (FileState::utf8(candidate)?, Some(warning)),
    };
    let mut preview = match &after {
        FileState::Utf8(text) => text.clone(),
        FileState::Missing => String::new(),
    };
    if let Some(warning) = &warning {
        preview = format!("Formatter warning: {warning}\n\n{preview}");
    }
    let token = uuid::Uuid::new_v4().to_string();
    let frozen = FrozenEdit {
        name: name.into(),
        arguments: args.clone(),
        path,
        before,
        after,
        warning,
    };
    let mut store = tickets().lock().unwrap_or_else(|p| p.into_inner());
    while store.entries.len() >= 128 {
        if let Some(old) = store.order.pop_front() {
            store.entries.remove(&old);
        }
    }
    store.entries.insert(token.clone(), frozen);
    store.order.push_back(token.clone());
    let object = args
        .as_object_mut()
        .ok_or("File edit arguments must be an object")?;
    object.insert(TOKEN.into(), Value::String(token));
    object.insert(PREVIEW.into(), Value::String(preview));
    Ok(())
}

async fn run(
    argv: &[String],
    input: &str,
    file: &Path,
    workspace: &crate::workspace::WorkspaceContext,
    sandbox: crate::sandbox::SandboxMode,
) -> Result<String, String> {
    let executable = if Path::new(&argv[0]).is_absolute() {
        PathBuf::from(&argv[0])
    } else {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|dir| dir.join(&argv[0]))
            .find(|path| path.is_file())
            .ok_or("formatter executable is not installed")?
    };
    let temporary = tempfile::tempdir().map_err(|_| "formatter temporary directory unavailable")?;
    let policy = crate::sandbox::SandboxPolicy {
        roots: vec![],
        read_only_roots: workspace.roots().iter().map(|r| r.path.clone()).collect(),
        temporary_directory: temporary.path().to_path_buf(),
        network_allowed: false,
        hide_other_processes: true,
    };
    let prepared = crate::sandbox::SandboxBackend::new(sandbox)
        .prepare_process_with_denied_paths(
            &executable,
            &argv[1..],
            &policy,
            &crate::jobs::automation_private_paths(),
        )
        .map_err(|_| "formatter sandbox unavailable")?;
    let mut command = tokio::process::Command::new(prepared.program);
    command
        .args(prepared.arguments)
        .current_dir(file.parent().unwrap_or(workspace.cwd()))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .env_clear();
    for key in [
        "HOME",
        "PATH",
        "USER",
        "LOGNAME",
        "LANG",
        "LC_ALL",
        "CARGO_HOME",
        "RUSTUP_HOME",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command.env("TMPDIR", temporary.path());
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().map_err(|_| "formatter could not start")?;
    struct Group(Option<u32>);
    impl Drop for Group {
        fn drop(&mut self) {
            #[cfg(unix)]
            if let Some(id) = self.0 {
                unsafe {
                    libc::kill(-(id as i32), libc::SIGKILL);
                }
            }
        }
    }
    let _group = Group(child.id());
    let mut stdin = child.stdin.take().ok_or("formatter stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("formatter stdout unavailable")?;
    let work = async {
        let write = async {
            stdin
                .write_all(input.as_bytes())
                .await
                .map_err(|_| "formatter stdin failed")?;
            stdin
                .shutdown()
                .await
                .map_err(|_| "formatter stdin failed")?;
            drop(stdin);
            Ok::<_, String>(())
        };
        let read = async {
            let mut bytes = Vec::new();
            stdout
                .take(crate::undo::MAX_UNDO_FILE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| "formatter output failed")?;
            if bytes.len() > crate::undo::MAX_UNDO_FILE_BYTES {
                return Err("formatter output exceeds file-size bound".into());
            }
            String::from_utf8(bytes).map_err(|_| "formatter returned invalid UTF-8".into())
        };
        let (_, output) = tokio::try_join!(write, read)?;
        if !child
            .wait()
            .await
            .map_err(|_| "formatter wait failed")?
            .success()
        {
            return Err("formatter exited unsuccessfully".into());
        }
        Ok(output)
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), work)
        .await
        .map_err(|_| "formatter timed out".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn orc290_custom_precedes_presets_and_selection_is_single_file() {
        let config = FormatterConfig {
            enabled: true,
            presets: vec!["rustfmt".into(), "ruff".into(), "prettier".into()],
            custom: vec![FormatterRule {
                extensions: vec!["rs".into()],
                command: vec!["custom".into()],
            }],
        };
        assert_eq!(select(&config, Path::new("a.rs")).unwrap(), vec!["custom"]);
        assert_eq!(select(&config, Path::new("a.py")).unwrap()[0], "ruff");
        assert!(select(&config, Path::new("a.bin")).is_none());
    }
    #[tokio::test]
    async fn orc290_filter_is_bounded_and_keeps_file_untouched() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("a.rs");
        std::fs::write(&path, "original").unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(temp.path()).unwrap();
        let output = run(
            &["/usr/bin/tr".into(), "a-z".into(), "A-Z".into()],
            "candidate",
            &path,
            &workspace,
            crate::sandbox::SandboxMode::PolicyOnly,
        )
        .await
        .unwrap();
        assert_eq!(output, "CANDIDATE");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
        assert!(run(
            &["/usr/bin/false".into()],
            "candidate",
            &path,
            &workspace,
            crate::sandbox::SandboxMode::PolicyOnly
        )
        .await
        .is_err());
    }
    #[test]
    fn orc290_frozen_edit_receipt_is_single_use_and_undo_restores_original() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("candidate.rs");
        std::fs::write(&path, "original").unwrap();
        let arguments = serde_json::json!({"path":path,"content":"candidate"});
        let token = uuid::Uuid::new_v4().to_string();
        tickets().lock().unwrap().entries.insert(
            token.clone(),
            FrozenEdit {
                name: "write_file".into(),
                arguments: arguments.clone(),
                path: path.clone(),
                before: FileState::Utf8("original".into()),
                after: FileState::Utf8("FORMATTED".into()),
                warning: None,
            },
        );
        let mut prepared = arguments;
        prepared[TOKEN] = Value::String(token);
        prepared[PREVIEW] = Value::String("FORMATTED".into());
        let (edit, _) = consume("write_file", &prepared).unwrap();
        assert!(consume("write_file", &prepared).is_err());
        let receipt = edit.apply().unwrap();
        assert_eq!(receipt.receipts().len(), 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "FORMATTED");
        let mut history = crate::undo::UndoStack::default();
        history.record_batch(receipt);
        let undo = history.prepare(crate::undo::UndoDirection::Undo).unwrap();
        crate::undo::perform(&undo).unwrap();
        assert!(history.complete(&undo));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
        let redo = history.prepare(crate::undo::UndoDirection::Redo).unwrap();
        crate::undo::perform(&redo).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "FORMATTED");
    }
    #[tokio::test]
    async fn orc290_formatter_rejects_invalid_and_oversized_output_and_missing_executable() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("candidate.txt");
        let workspace = crate::workspace::WorkspaceContext::discover(temp.path()).unwrap();
        assert!(run(
            &["/usr/bin/yes".into()],
            "candidate",
            &file,
            &workspace,
            crate::sandbox::SandboxMode::PolicyOnly
        )
        .await
        .unwrap_err()
        .contains("bound"));
        assert!(run(
            &["/nonexistent/orc290-formatter".into()],
            "candidate",
            &file,
            &workspace,
            crate::sandbox::SandboxMode::PolicyOnly
        )
        .await
        .is_err());
        assert!(run(
            &["/usr/bin/printf".into(), "\\377".into()],
            "candidate",
            &file,
            &workspace,
            crate::sandbox::SandboxMode::PolicyOnly
        )
        .await
        .unwrap_err()
        .contains("UTF-8"));
        assert!(!file.exists());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn orc290_cancellation_stops_formatter_and_publishes_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("candidate.txt");
        let pid_file = temp.path().join("formatter.pid");
        let workspace = crate::workspace::WorkspaceContext::discover(temp.path()).unwrap();
        let task_file = file.clone();
        let task_pid = pid_file.clone();
        let task = tokio::spawn(async move {
            run(
                &[
                    "/bin/sh".into(),
                    "-c".into(),
                    "echo $$ > \"$1\"; exec sleep 30".into(),
                    "orc290".into(),
                    task_pid.to_string_lossy().into_owned(),
                ],
                "candidate",
                &task_file,
                &workspace,
                crate::sandbox::SandboxMode::PolicyOnly,
            )
            .await
        });
        let pid = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Ok(text) = tokio::fs::read_to_string(&pid_file).await {
                    if let Ok(pid) = text.trim().parse::<i32>() {
                        break pid;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while unsafe { libc::kill(pid, 0) } == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(!file.exists());
    }
    #[tokio::test]
    async fn orc290_drift_after_formatting_does_not_overwrite_new_contents() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("candidate.rs");
        std::fs::write(&path, "original").unwrap();
        let prepared = crate::undo::PreparedFileMutation::single(
            path.clone(),
            "write_file",
            FileState::Utf8("original".into()),
            FileState::Utf8("FORMATTED".into()),
        )
        .unwrap();
        std::fs::write(&path, "concurrent edit").unwrap();
        assert!(prepared.apply().is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "concurrent edit");
    }
    #[tokio::test]
    async fn orc290_timeout_leaves_candidate_unpublished() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("candidate.txt");
        let workspace = crate::workspace::WorkspaceContext::discover(temp.path()).unwrap();
        let started = std::time::Instant::now();
        let result = run(
            &["/bin/sleep".into(), "30".into()],
            "candidate",
            &file,
            &workspace,
            crate::sandbox::SandboxMode::PolicyOnly,
        )
        .await;
        assert!(result.unwrap_err().contains("timed out"));
        assert!(started.elapsed() < std::time::Duration::from_secs(15));
        assert!(!file.exists());
    }
}
