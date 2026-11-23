use crate::agent::client::ToolDef;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

pub const SNAPSHOT_LIMIT: usize = 4 * 1024;
pub const WRAP_UP: &str = "Local finish wrap-up: change approach once, then report what you have.";
pub const REPORT: &str = "Local finish stopped tools. Report what you have.";
pub const CONTINUE_TODO: &str = "A todo is still open. Continue that open todo once.";
pub const BACKGROUND_HINT: &str = "Refused a foreground server. Run it in the background.";
const READ_FILE_SHORT: &str = "Read a line range from a file.";

tokio::task_local! {
    static FINISH: Arc<Mutex<LocalFinish>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlainAction {
    ContinueOnce,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncidentAction {
    Pass,
    WrapUp,
    Stop,
}

#[derive(Debug, Clone, Default)]
pub struct LocalFinish {
    local: bool,
    snapshot_sent: bool,
    needs_todo: bool,
    todos_written: bool,
    open_todos: bool,
    plain_continuations: u8,
    nudges: u8,
    tools_stopped: bool,
    check_command: Option<String>,
    mtimes: HashMap<PathBuf, u64>,
    last_task: String,
}

impl LocalFinish {
    pub fn user_prefix(
        &mut self,
        local: bool,
        root: &Path,
        task: &str,
        check_command: Option<&str>,
    ) -> String {
        self.local = local;
        self.last_task = task.to_string();
        self.needs_todo = local && names_three_steps(task);
        self.check_command = check_command
            .map(str::trim)
            .filter(|command| !command.is_empty())
            .map(str::to_string);
        if !local {
            return String::new();
        }
        if !self.snapshot_sent {
            self.snapshot_sent = true;
            self.mtimes = scan_mtimes(root);
            return snapshot(root);
        }
        let changed = self.changed_outside(root);
        self.mtimes = scan_mtimes(root);
        if changed.is_empty() {
            return String::new();
        }
        let mut out = String::from("Changed outside the turn:\n");
        for path in changed {
            if out.len() >= SNAPSHOT_LIMIT {
                break;
            }
            out.push_str(&path);
            out.push('\n');
        }
        out
    }

    pub fn admit_edit(&self) -> Result<(), String> {
        if !self.local {
            return Ok(());
        }
        if self.tools_stopped {
            return Err(REPORT.into());
        }
        if self.needs_todo && !self.todos_written {
            return Err("Write the steps with todo_write before editing.".into());
        }
        Ok(())
    }

    pub fn note_todos(&mut self, open: bool) {
        if !self.local {
            return;
        }
        self.todos_written = true;
        self.open_todos = open;
    }

    pub fn on_plain_reply(&mut self) -> PlainAction {
        if !self.local || !self.open_todos || self.plain_continuations >= 1 {
            return PlainAction::End;
        }
        self.plain_continuations = self.plain_continuations.saturating_add(1);
        PlainAction::ContinueOnce
    }

    pub fn admit_command(&self, command: &str, background: bool) -> Result<(), String> {
        if !self.local {
            return Ok(());
        }
        if self.tools_stopped {
            return Err(REPORT.into());
        }
        if !background && foreground_server(command) {
            return Err(BACKGROUND_HINT.into());
        }
        Ok(())
    }

    pub fn on_incident(&mut self) -> IncidentAction {
        if !self.local {
            return IncidentAction::Pass;
        }
        if self.nudges == 0 {
            self.nudges = 1;
            return IncidentAction::WrapUp;
        }
        self.tools_stopped = true;
        IncidentAction::Stop
    }

    pub fn check_command(&self) -> Option<&str> {
        if !self.local {
            return None;
        }
        self.check_command.as_deref()
    }

    fn changed_outside(&self, root: &Path) -> Vec<String> {
        let mut changed = Vec::new();
        for (path, mtime) in scan_mtimes(root) {
            let previous = self.mtimes.get(&path).copied();
            if previous != Some(mtime) {
                if let Ok(relative) = path.strip_prefix(root) {
                    changed.push(relative.display().to_string());
                }
            }
        }
        changed.sort();
        changed
    }
}

pub fn prefix_text(content: &str, prefix: &str) -> String {
    if prefix.is_empty() || content.starts_with(prefix) {
        content.to_string()
    } else {
        format!("{prefix}{content}")
    }
}

pub fn attach_replan(prompts: &mut Vec<String>, replan: Option<&str>) {
    let Some(replan) = replan else {
        return;
    };
    if prompts.iter().any(|item| item == replan) {
        return;
    }
    prompts.push(replan.to_string());
}

pub fn baseline_turn(finish: &Mutex<LocalFinish>, root: &Path) {
    let mut guard = finish.lock().unwrap_or_else(|poison| poison.into_inner());
    if guard.local {
        guard.mtimes = scan_mtimes(root);
    }
}

pub fn prepare_local_user_message(
    finish: &Mutex<LocalFinish>,
    local: bool,
    root: &Path,
    task: &str,
    check_command: Option<&str>,
) -> String {
    finish
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .user_prefix(local, root, task, check_command)
}

pub async fn scope<T>(
    finish: Arc<Mutex<LocalFinish>>,
    future: impl std::future::Future<Output = T>,
) -> T {
    FINISH.scope(finish, future).await
}

pub fn note_todos(open: bool) {
    let Some(finish) = current() else {
        return;
    };
    finish
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .note_todos(open);
}

pub fn admit_edit() -> Result<(), String> {
    let Some(finish) = current() else {
        return Ok(());
    };
    let guard = finish.lock().unwrap_or_else(|poison| poison.into_inner());
    guard.admit_edit()
}

pub fn admit_command(command: &str, background: bool) -> Result<(), String> {
    let Some(finish) = current() else {
        return Ok(());
    };
    let guard = finish.lock().unwrap_or_else(|poison| poison.into_inner());
    guard.admit_command(command, background)
}

pub fn check_tail(cwd: &Path) -> Option<String> {
    let finish = current()?;
    let command = finish
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .check_command()
        .map(str::to_string)?;
    run_check(&command, cwd)
}

pub fn run_check(command: &str, cwd: &Path) -> Option<String> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .output()
        .ok()?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let tail: String = text
        .chars()
        .rev()
        .take(2048)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    Some(tail)
}

pub fn apply_local_schemas(local: bool, tools: &mut [ToolDef]) {
    if !local {
        return;
    }
    for tool in tools {
        if !tool.function.name.starts_with("mcp") {
            tool.function.description =
                short_description(&tool.function.name, &tool.function.description);
        }
        let schema = serde_json::to_string(&tool.function.parameters).unwrap_or_default();
        if schema.len() > 2048 {
            tool.function.description = format!("{}. Use search_mcp_tools.", tool.function.name);
            tool.function.parameters = serde_json::json!({"type": "object", "properties": {}});
        }
    }
}

fn short_description(name: &str, description: &str) -> String {
    if name == "read_file" {
        return READ_FILE_SHORT.into();
    }
    let cut = description.split([';', '\n']).next().unwrap_or(description);
    let mut short: String = cut.chars().take(96).collect();
    if short.chars().count() < cut.chars().count() {
        short.push('…');
    }
    short
}

fn current() -> Option<Arc<Mutex<LocalFinish>>> {
    FINISH.try_with(Arc::clone).ok()
}

pub fn names_three_steps(task: &str) -> bool {
    let numbered = task
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            let mut chars = trimmed.chars();
            matches!(chars.next(), Some('1'..='9')) && matches!(chars.next(), Some('.' | ')'))
        })
        .count();
    if numbered >= 3 {
        return true;
    }
    let lower = task.to_ascii_lowercase();
    lower.contains("three steps") || lower.contains("3 steps")
}

fn foreground_server(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    [
        "npm run dev",
        "npm start",
        "pnpm dev",
        "yarn dev",
        "bun dev",
        "next dev",
        "vite",
        "flask run",
        "uvicorn",
        "http.server",
        "rails server",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn snapshot(root: &Path) -> String {
    let mut names = top_level_names(root);
    names.sort();
    let mut out = String::from("Project snapshot:\n");
    for name in names {
        if out.len().saturating_add(name.len()).saturating_add(1) > SNAPSHOT_LIMIT {
            break;
        }
        out.push_str(&name);
        out.push('\n');
    }
    out
}

fn top_level_names(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return names;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" || name == "target" || name == "node_modules" {
            continue;
        }
        names.push(name);
    }
    names
}

fn scan_mtimes(root: &Path) -> HashMap<PathBuf, u64> {
    let mut found = HashMap::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".git" || name == "target" || name == "node_modules" {
            continue;
        }
        if let Ok(metadata) = std::fs::metadata(&path) {
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|time| time.as_secs())
                .unwrap_or(0);
            found.insert(path, mtime);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::{ToolDef, ToolFunction};

    fn tool(name: &str, description: &str, parameters: serde_json::Value) -> ToolDef {
        ToolDef {
            def_type: "function".into(),
            function: ToolFunction {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }

    #[tokio::test]
    async fn local_finish_rule_snapshot_todo_check_server_and_nudge() {
        let root = tempfile::tempdir().unwrap();
        let body = "UNIQUE_BODY_ZZZ_NOT_A_NAME";
        std::fs::write(root.path().join("notes.txt"), body).unwrap();
        let task = "1. read\n2. edit\n3. check\n";
        let finish = Arc::new(Mutex::new(LocalFinish::default()));
        let prefix = prepare_local_user_message(&finish, true, root.path(), task, None);
        let message = prefix_text(task, &prefix);
        println!("snapshot={prefix}");
        println!("message={message}");
        assert!(message.starts_with("Project snapshot:"));
        assert!(message.contains("notes.txt"));
        assert!(message.contains(task.trim()));
        assert!(!message.contains(body));
        assert!(prefix.len() <= SNAPSHOT_LIMIT);
        std::fs::write(root.path().join("turn.txt"), "written during the turn").unwrap();
        baseline_turn(&finish, root.path());
        let during = prepare_local_user_message(&finish, true, root.path(), task, None);
        println!("during={during:?}");
        assert!(!during.contains("turn.txt"));
        scope(finish.clone(), async {
            let rejected = admit_edit().unwrap_err();
            println!("rejected={rejected}");
            assert!(rejected.contains("todo_write"));
            note_todos(true);
            assert!(admit_edit().is_ok());
            assert!(check_tail(root.path()).is_none());
        })
        .await;
        assert!(!root.path().join("ran-check").exists());
        assert_eq!(
            finish.lock().unwrap().on_plain_reply(),
            PlainAction::ContinueOnce
        );
        let mut continued = Vec::new();
        attach_replan(&mut continued, Some(CONTINUE_TODO));
        println!("continue={}", continued[0]);
        assert_eq!(continued, vec![CONTINUE_TODO.to_string()]);
        attach_replan(&mut continued, Some(CONTINUE_TODO));
        assert_eq!(continued.len(), 1);
        assert_eq!(finish.lock().unwrap().on_plain_reply(), PlainAction::End);
        std::process::Command::new("touch")
            .args(["-t", "202001010101.01", "notes.txt"])
            .current_dir(root.path())
            .status()
            .unwrap();
        let later = prepare_local_user_message(
            &finish,
            true,
            root.path(),
            "follow up",
            Some("printf 'check-tail-marker' && touch ran-check"),
        );
        let later_message = prefix_text("follow up", &later);
        println!("later={later}");
        println!("later-message={later_message}");
        assert!(later_message.starts_with("Changed outside the turn:"));
        assert!(later_message.contains("notes.txt"));
        assert!(!later_message.contains("turn.txt"));
        assert!(!later_message.contains(body));
        assert!(!later_message.contains("Project snapshot:"));
        let tail = scope(finish.clone(), async { check_tail(root.path()) }).await;
        let tail = tail.unwrap();
        println!("check-tail={tail}");
        assert!(tail.contains("check-tail-marker"));
        assert!(root.path().join("ran-check").exists());
        let refused = scope(finish.clone(), async {
            admit_command("npm run dev", false)
        })
        .await
        .unwrap_err();
        println!("refused={refused}");
        assert!(refused.contains("background"));
        assert!(
            scope(finish.clone(), async { admit_command("npm run dev", true) })
                .await
                .is_ok()
        );
        assert_eq!(finish.lock().unwrap().on_incident(), IncidentAction::WrapUp);
        let mut prompts = Vec::new();
        attach_replan(&mut prompts, Some(WRAP_UP));
        println!("wrap-up={}", prompts[0]);
        assert_eq!(finish.lock().unwrap().on_incident(), IncidentAction::Stop);
        println!("report={REPORT}");
        let stopped = scope(finish, async { admit_edit() }).await.unwrap_err();
        assert_eq!(stopped, REPORT);
    }

    #[tokio::test]
    async fn local_finish_cloud_turn_has_no_snapshot_or_nudge() {
        let root = tempfile::tempdir().unwrap();
        let body = "UNIQUE_BODY_ZZZ_NOT_A_NAME";
        std::fs::write(root.path().join("notes.txt"), body).unwrap();
        let task = "1. a\n2. b\n3. c\n";
        let finish = Arc::new(Mutex::new(LocalFinish::default()));
        let prefix = prepare_local_user_message(&finish, false, root.path(), task, None);
        let message = prefix_text(task, &prefix);
        println!(
            "cloud-prefix={prefix:?} message={message:?} incident={:?}",
            finish.lock().unwrap().on_incident()
        );
        assert!(prefix.is_empty());
        assert_eq!(message, task);
        assert!(!message.contains("Project snapshot:"));
        assert!(!message.contains(WRAP_UP));
        assert!(!message.contains(body));
        scope(finish.clone(), async {
            assert!(admit_edit().is_ok());
            assert!(admit_command("npm run dev", false).is_ok());
            assert!(check_tail(root.path()).is_none());
        })
        .await;
        assert_eq!(finish.lock().unwrap().on_incident(), IncidentAction::Pass);
        assert_eq!(finish.lock().unwrap().on_plain_reply(), PlainAction::End);
    }

    #[test]
    fn local_schema_shortens_builtin_and_mcp_only_on_managed_route() {
        let registry = crate::agent::registry::ToolRegistry::builtins();
        let mut local = registry.definitions(crate::agent::registry::ToolProfile::Main);
        local.push(tool(
            "mcp_huge",
            "large schema",
            serde_json::json!({"type":"object","blob": "y".repeat(3000)}),
        ));
        let mut cloud = local.clone();
        apply_local_schemas(false, &mut cloud);
        let cloud_read = cloud
            .iter()
            .find(|item| item.function.name == "read_file")
            .unwrap();
        assert!(cloud_read
            .function
            .description
            .contains("exact 8-hex content anchor for hashline_edit"));
        assert!(!cloud
            .iter()
            .any(|item| item.function.description.contains("search_mcp_tools")));
        apply_local_schemas(true, &mut local);
        let read = local
            .iter()
            .find(|item| item.function.name == "read_file")
            .unwrap();
        println!(
            "short={} long-still={}",
            read.function.description, cloud_read.function.description
        );
        assert_eq!(read.function.description, READ_FILE_SHORT);
        assert!(!read
            .function
            .description
            .contains("exact 8-hex content anchor for hashline_edit"));
        assert!(local
            .iter()
            .filter(|item| !item.function.name.starts_with("mcp"))
            .all(|item| item.function.description.chars().count() <= 97));
        let mcp = local
            .iter()
            .find(|item| item.function.name == "mcp_huge")
            .unwrap();
        assert!(mcp.function.description.contains("search_mcp_tools"));
        assert!(
            serde_json::to_string(&mcp.function.parameters)
                .unwrap()
                .len()
                < 2048
        );
    }
}
