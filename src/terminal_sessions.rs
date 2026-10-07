use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use thiserror::Error;
use tokio::process::Command;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_LIST_BYTES: usize = 64 * 1024;
const MAX_SESSIONS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSession {
    pub name: String,
    pub windows: u32,
    pub attached_clients: u32,
    pub created_unix: u64,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TerminalSessionError {
    #[error("tmux is not installed or is not available on PATH")]
    Unavailable,
    #[error("terminal session name must start with a letter or digit and use at most 64 letters, digits, '.', '_' or '-'")]
    InvalidName,
    #[error("terminal working directory is unavailable")]
    InvalidDirectory,
    #[error("tmux did not respond within 3 seconds")]
    TimedOut,
    #[error("tmux returned an invalid or incomplete session list")]
    InvalidResponse,
    #[error("tmux command failed{0}")]
    CommandFailed(String),
}

#[derive(Debug, Clone)]
pub struct TmuxManager {
    executable: PathBuf,
}

impl Default for TmuxManager {
    fn default() -> Self {
        Self::new("tmux")
    }
}

impl TmuxManager {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    pub async fn version(&self) -> Result<String, TerminalSessionError> {
        let output = self.run(&["-V"]).await?;
        let version = bounded_utf8(&output.stdout, 256).trim().to_string();
        if version.is_empty() {
            Err(TerminalSessionError::InvalidResponse)
        } else {
            Ok(version)
        }
    }

    pub async fn list(&self) -> Result<Vec<TerminalSession>, TerminalSessionError> {
        let output = self
            .run_allow_empty(&[
                "list-sessions",
                "-F",
                "#{session_name}\t#{session_windows}\t#{session_attached}\t#{session_created}",
            ])
            .await?;
        if !output.status.success() {
            let stderr = bounded_utf8(&output.stderr, 512).to_ascii_lowercase();
            if stderr.contains("no server running") || stderr.contains("no sessions") {
                return Ok(Vec::new());
            }
            return Err(command_failed(output.status));
        }
        if output.stdout.len() > MAX_LIST_BYTES {
            return Err(TerminalSessionError::InvalidResponse);
        }
        let text = std::str::from_utf8(&output.stdout)
            .map_err(|_| TerminalSessionError::InvalidResponse)?;
        let mut sessions = Vec::new();
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            if sessions.len() == MAX_SESSIONS {
                return Err(TerminalSessionError::InvalidResponse);
            }
            let mut fields = line.split('\t');
            let name = fields.next().unwrap_or_default();
            let windows = fields.next().and_then(|value| value.parse().ok());
            let attached_clients = fields.next().and_then(|value| value.parse().ok());
            let created_unix = fields.next().and_then(|value| value.parse().ok());
            if fields.next().is_some()
                || !valid_existing_name(name)
                || windows.is_none()
                || attached_clients.is_none()
                || created_unix.is_none()
            {
                return Err(TerminalSessionError::InvalidResponse);
            }
            sessions.push(TerminalSession {
                name: name.to_string(),
                windows: windows.expect("checked"),
                attached_clients: attached_clients.expect("checked"),
                created_unix: created_unix.expect("checked"),
            });
        }
        sessions.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(sessions)
    }

    pub async fn create(&self, name: &str, cwd: &Path) -> Result<(), TerminalSessionError> {
        validate_name(name)?;
        if !cwd.is_dir() {
            return Err(TerminalSessionError::InvalidDirectory);
        }
        let cwd = cwd.to_str().ok_or(TerminalSessionError::InvalidDirectory)?;
        self.run(&["new-session", "-d", "-s", name, "-c", cwd])
            .await
            .map(|_| ())
    }

    pub async fn detach(&self, name: &str) -> Result<(), TerminalSessionError> {
        validate_existing_name(name)?;
        let target = format!("={name}");
        self.run(&["detach-client", "-s", &target])
            .await
            .map(|_| ())
    }

    pub async fn kill(&self, name: &str) -> Result<(), TerminalSessionError> {
        validate_existing_name(name)?;
        let target = format!("={name}");
        self.run(&["kill-session", "-t", &target]).await.map(|_| ())
    }

    pub fn attach(&self, name: &str) -> Result<ExitStatus, TerminalSessionError> {
        validate_existing_name(name)?;
        let target = format!("={name}");
        let mut command = std::process::Command::new(&self.executable);
        command.args(["attach-session", "-t", &target]);
        crate::run_environment::apply_std(&mut command);
        command.status().map_err(map_spawn_error)
    }

    async fn run(&self, args: &[&str]) -> Result<std::process::Output, TerminalSessionError> {
        let output = self.run_allow_empty(args).await?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(command_failed(output.status))
        }
    }

    async fn run_allow_empty(
        &self,
        args: &[&str],
    ) -> Result<std::process::Output, TerminalSessionError> {
        let mut command = Command::new(&self.executable);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        crate::run_environment::apply_tokio(&mut command);
        tokio::time::timeout(COMMAND_TIMEOUT, command.output())
            .await
            .map_err(|_| TerminalSessionError::TimedOut)?
            .map_err(map_spawn_error)
    }
}

pub fn valid_session_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn validate_name(name: &str) -> Result<(), TerminalSessionError> {
    valid_session_name(name)
        .then_some(())
        .ok_or(TerminalSessionError::InvalidName)
}

fn valid_existing_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.chars().any(char::is_control)
        && !name.contains('\t')
}

fn validate_existing_name(name: &str) -> Result<(), TerminalSessionError> {
    valid_existing_name(name)
        .then_some(())
        .ok_or(TerminalSessionError::InvalidName)
}

fn map_spawn_error(error: std::io::Error) -> TerminalSessionError {
    if error.kind() == std::io::ErrorKind::NotFound {
        TerminalSessionError::Unavailable
    } else {
        TerminalSessionError::CommandFailed(String::new())
    }
}

fn command_failed(status: ExitStatus) -> TerminalSessionError {
    let suffix = status
        .code()
        .map(|code| format!(" (exit {code})"))
        .unwrap_or_default();
    TerminalSessionError::CommandFailed(suffix)
}

fn bounded_utf8(bytes: &[u8], limit: usize) -> String {
    let end = bytes.len().min(limit);
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fake_tmux(script_body: &str) -> (tempfile::TempDir, TmuxManager, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let script = root.path().join("tmux");
        let log = root.path().join("argv.log");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n{script_body}\n",
                log.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();
        (root, TmuxManager::new(script), log)
    }

    #[test]
    fn names_are_conservative_and_bounded() {
        for valid in ["dev", "ca-1", "a.b_c", "Z9"] {
            assert!(valid_session_name(valid), "{valid}");
        }
        for invalid in ["", "-bad", "has space", "bad:target", "../escape"] {
            assert!(!valid_session_name(invalid), "{invalid}");
        }
        assert!(!valid_session_name(&"a".repeat(65)));
    }

    #[tokio::test]
    async fn lists_sorted_safe_summaries() {
        let (_root, manager, _log) =
            fake_tmux("printf 'zeta\\t2\\t0\\t20\\nalpha team\\t1\\t3\\t10\\n'");
        let sessions = manager.list().await.unwrap();
        assert_eq!(sessions[0].name, "alpha team");
        assert_eq!(sessions[0].attached_clients, 3);
        assert_eq!(sessions[1].windows, 2);
    }

    #[tokio::test]
    async fn create_detach_and_kill_use_exact_argv() {
        let (root, manager, log) = fake_tmux("exit 0");
        manager.create("dev-1", root.path()).await.unwrap();
        manager.detach("dev-1").await.unwrap();
        manager.kill("dev-1").await.unwrap();
        let log = std::fs::read_to_string(log).unwrap();
        assert_eq!(
            log.lines().collect::<Vec<_>>(),
            [
                format!("new-session -d -s dev-1 -c {}", root.path().display()),
                "detach-client -s =dev-1".into(),
                "kill-session -t =dev-1".into(),
            ]
        );
    }

    #[tokio::test]
    async fn missing_binary_is_safe_and_body_free() {
        let manager = TmuxManager::new("/definitely/missing/ca-tmux");
        assert_eq!(
            manager.list().await.unwrap_err(),
            TerminalSessionError::Unavailable
        );
    }

    #[tokio::test]
    async fn malformed_or_oversized_lists_fail_closed() {
        let (_root, manager, _log) = fake_tmux("printf 'bad row\\n'");
        assert_eq!(
            manager.list().await.unwrap_err(),
            TerminalSessionError::InvalidResponse
        );

        let body = format!(
            "python3 - <<'PY'\nprint('a\\t1\\t0\\t1\\n' * {})\nPY",
            MAX_SESSIONS + 1
        );
        let (_root, manager, _log) = fake_tmux(&body);
        assert_eq!(
            manager.list().await.unwrap_err(),
            TerminalSessionError::InvalidResponse
        );
    }

    #[tokio::test]
    async fn no_server_is_an_empty_list() {
        let (_root, manager, _log) = fake_tmux("printf 'no server running' >&2\nexit 1");
        assert!(manager.list().await.unwrap().is_empty());
    }

    #[test]
    fn attach_uses_exact_target_and_returns_status() {
        let (_root, manager, log) = fake_tmux("exit 0");
        assert!(manager.attach("dev").unwrap().success());
        assert_eq!(
            std::fs::read_to_string(log).unwrap(),
            "attach-session -t =dev\n"
        );
    }
}
