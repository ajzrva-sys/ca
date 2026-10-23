use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncReadExt;

pub const STATUS_COMMAND_TIMEOUT: Duration = Duration::from_secs(1);
pub const STATUS_COMMAND_OUTPUT_BYTES: usize = 4 * 1024;
const STATUS_COMMAND_CONTENT_BYTES: usize = STATUS_COMMAND_OUTPUT_BYTES - 64;
pub const DEFAULT_STATUS_REFRESH_SECS: u64 = 5;
const MAX_STATUS_REFRESH_SECS: u64 = 3_600;
const MAX_STATUS_COMMAND_BYTES: usize = 1_024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "kebab-case")]
pub enum UiTheme {
    /// Neutral charcoal conversation surfaces with purple agent identities.
    #[default]
    Default,
    Ocean,
    Amber,
    Mimocode,
    Mono,
    RosePine,
}

impl UiTheme {
    pub const ALL: [Self; 6] = [
        Self::Default,
        Self::Ocean,
        Self::Amber,
        Self::Mimocode,
        Self::Mono,
        Self::RosePine,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Ocean => "Ocean",
            Self::Amber => "Amber",
            Self::Mimocode => "Mimocode",
            Self::Mono => "Mono",
            Self::RosePine => "Rose Pine",
        }
    }

    pub const fn config_value(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Ocean => "ocean",
            Self::Amber => "amber",
            Self::Mimocode => "mimocode",
            Self::Mono => "mono",
            Self::RosePine => "rose-pine",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum NotificationStyle {
    #[default]
    Off,
    Bel,
    Osc9,
}

impl NotificationStyle {
    pub const ALL: [Self; 3] = [Self::Off, Self::Bel, Self::Osc9];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Bel => "BEL",
            Self::Osc9 => "OSC 9",
        }
    }

    pub const fn config_value(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Bel => "bel",
            Self::Osc9 => "osc9",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum MouseMode {
    #[default]
    Scroll,
    Off,
}

impl MouseMode {
    pub const ALL: [Self; 2] = [Self::Scroll, Self::Off];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Scroll => "Scroll",
            Self::Off => "Off",
        }
    }

    pub const fn config_value(self) -> &'static str {
        match self {
            Self::Scroll => "scroll",
            Self::Off => "off",
        }
    }

    pub const fn toggled(self) -> Self {
        match self {
            Self::Scroll => Self::Off,
            Self::Off => Self::Scroll,
        }
    }
}

pub fn notification_sequence(style: NotificationStyle, passthrough: bool) -> Option<String> {
    match style {
        NotificationStyle::Off => None,
        NotificationStyle::Bel => Some("\u{7}".into()),
        NotificationStyle::Osc9 => {
            let osc9 = "\u{1b}]9;ca: ready\u{7}";
            Some(if passthrough {
                format!("\u{1b}Ptmux;\u{1b}{osc9}\u{1b}\\")
            } else {
                osc9.into()
            })
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StatusField {
    Messages,
    Tokens,
    Model,
    Profile,
    Mode,
    Workspace,
    GitBranch,
    GenerationSpeed,
    SittingCost,
    Custom,
    SendHint,
    HistoryHint,
    CommandsHint,
}

impl StatusField {
    pub const ALL: [Self; 13] = [
        Self::Messages,
        Self::Tokens,
        Self::Model,
        Self::Profile,
        Self::Mode,
        Self::Workspace,
        Self::GitBranch,
        Self::GenerationSpeed,
        Self::SittingCost,
        Self::Custom,
        Self::SendHint,
        Self::HistoryHint,
        Self::CommandsHint,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Messages => "Messages",
            Self::Tokens => "Tokens",
            Self::Model => "Model",
            Self::Profile => "Profile",
            Self::Mode => "Mode",
            Self::Workspace => "Workspace",
            Self::GitBranch => "Git branch",
            Self::GenerationSpeed => "Generation speed",
            Self::SittingCost => "Sitting cost",
            Self::Custom => "Custom command",
            Self::SendHint => "Send hint",
            Self::HistoryHint => "History hint",
            Self::CommandsHint => "Commands hint",
        }
    }

    pub const fn config_value(self) -> &'static str {
        match self {
            Self::Messages => "messages",
            Self::Tokens => "tokens",
            Self::Model => "model",
            Self::Profile => "profile",
            Self::Mode => "mode",
            Self::Workspace => "workspace",
            Self::GitBranch => "git_branch",
            Self::GenerationSpeed => "generation_speed",
            Self::SittingCost => "sitting_cost",
            Self::Custom => "custom",
            Self::SendHint => "send_hint",
            Self::HistoryHint => "history_hint",
            Self::CommandsHint => "commands_hint",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TranscriptConfig {
    #[serde(default)]
    pub timestamps: bool,
    #[serde(default)]
    pub assistant_footers: bool,
}

fn default_status_fields() -> Vec<StatusField> {
    vec![
        StatusField::Messages,
        StatusField::Tokens,
        StatusField::SendHint,
        StatusField::HistoryHint,
        StatusField::CommandsHint,
    ]
}

fn default_refresh_secs() -> u64 {
    DEFAULT_STATUS_REFRESH_SECS
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatuslineConfig {
    #[serde(default = "default_status_fields")]
    pub fields: Vec<StatusField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_command: Option<String>,
    #[serde(default)]
    pub custom_enabled: bool,
    #[serde(default = "default_refresh_secs")]
    pub refresh_secs: u64,
}

impl Default for StatuslineConfig {
    fn default() -> Self {
        Self {
            fields: default_status_fields(),
            custom_command: None,
            custom_enabled: false,
            refresh_secs: DEFAULT_STATUS_REFRESH_SECS,
        }
    }
}

impl std::fmt::Debug for StatuslineConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StatuslineConfig")
            .field("fields", &self.fields)
            .field(
                "custom_command",
                &self.custom_command.as_ref().map(|_| "[REDACTED]"),
            )
            .field("custom_enabled", &self.custom_enabled)
            .field("refresh_secs", &self.refresh_secs)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnimationConfig {
    #[serde(default = "default_animation_fps")]
    pub fps: u8,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            fps: default_animation_fps(),
        }
    }
}

fn default_animation_fps() -> u8 {
    30
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum BulletStyle {
    #[default]
    Diamond,
    Dot,
    SmallCircle,
    Circle,
    SmallTriangle,
    Triangle,
    None,
}

impl BulletStyle {
    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Diamond => "◆",
            Self::Dot => "·",
            Self::SmallCircle => "•",
            Self::Circle => "●",
            Self::SmallTriangle => "▸",
            Self::Triangle => "▶",
            Self::None => "",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolBlockStyle {
    #[serde(default)]
    pub muted_collapsed: bool,
    #[serde(default)]
    pub dim_details: bool,
    #[serde(default)]
    pub bullet: BulletStyle,
}

impl Default for ToolBlockStyle {
    fn default() -> Self {
        Self {
            muted_collapsed: true,
            dim_details: true,
            bullet: BulletStyle::Diamond,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThinkingBlockStyle {
    #[serde(default = "default_true")]
    pub accent_enabled: bool,
    #[serde(default = "default_true")]
    pub animate: bool,
    #[serde(default = "default_three")]
    pub truncated_lines: usize,
}

impl Default for ThinkingBlockStyle {
    fn default() -> Self {
        Self {
            accent_enabled: true,
            animate: true,
            truncated_lines: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EditBlockStyle {
    #[serde(default = "default_true")]
    pub indent: bool,
    #[serde(default)]
    pub dual_line_numbers: bool,
    #[serde(default = "default_hunk_separator")]
    pub hunk_separator: String,
}

fn default_hunk_separator() -> String {
    "…".into()
}

fn default_true() -> bool {
    true
}

fn default_three() -> usize {
    3
}

impl Default for EditBlockStyle {
    fn default() -> Self {
        Self {
            indent: true,
            dual_line_numbers: false,
            hunk_separator: default_hunk_separator(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockStyleConfig {
    #[serde(default)]
    pub tool: ToolBlockStyle,
    #[serde(default)]
    pub thinking: ThinkingBlockStyle,
    #[serde(default)]
    pub edit: EditBlockStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScrollbarConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub gap_left: u16,
    #[serde(default)]
    pub gap_right: u16,
}

impl Default for ScrollbarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            gap_left: 0,
            gap_right: 0,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct UiConfig {
    #[serde(default)]
    pub theme: UiTheme,
    #[serde(default)]
    pub notification: NotificationStyle,
    #[serde(default)]
    pub status_indicators: bool,
    #[serde(default)]
    pub mouse: MouseMode,
    #[serde(default)]
    pub statusline: StatuslineConfig,
    #[serde(default)]
    pub transcript: TranscriptConfig,
    #[serde(default)]
    pub animation: AnimationConfig,
    #[serde(default)]
    pub compact_mode: bool,
    #[serde(default)]
    pub blocks: BlockStyleConfig,
    #[serde(default)]
    pub auto_dark_theme: Option<UiTheme>,
    #[serde(default)]
    pub auto_light_theme: Option<UiTheme>,
    #[serde(default)]
    pub scrollbar: ScrollbarConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatuslineUpdate {
    pub fields: Vec<StatusField>,
    pub custom_enabled: bool,
}

impl std::fmt::Debug for UiConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UiConfig")
            .field("theme", &self.theme)
            .field("notification", &self.notification)
            .field("status_indicators", &self.status_indicators)
            .field("mouse", &self.mouse)
            .field("statusline", &self.statusline)
            .field("transcript", &self.transcript)
            .finish()
    }
}

impl UiConfig {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn validate(&self) -> Result<(), UiConfigError> {
        let mut seen = std::collections::HashSet::new();
        if self
            .statusline
            .fields
            .iter()
            .any(|field| !seen.insert(*field))
        {
            return Err(UiConfigError::DuplicateField);
        }
        if self.statusline.refresh_secs == 0
            || self.statusline.refresh_secs > MAX_STATUS_REFRESH_SECS
        {
            return Err(UiConfigError::InvalidRefreshInterval);
        }
        if let Some(command) = self.statusline.custom_command.as_deref() {
            if command.trim().is_empty()
                || command.len() > MAX_STATUS_COMMAND_BYTES
                || command
                    .chars()
                    .any(|ch| ch == '\0' || ch == '\r' || ch == '\n')
            {
                return Err(UiConfigError::InvalidCustomCommand);
            }
        }
        if self.statusline.custom_enabled && self.statusline.custom_command.is_none() {
            return Err(UiConfigError::MissingCustomCommand);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum UiConfigError {
    #[error("statusline fields must not contain duplicates")]
    DuplicateField,
    #[error("statusline refresh must be between 1 and 3600 seconds")]
    InvalidRefreshInterval,
    #[error("custom status command must be one non-empty line of at most 1024 bytes")]
    InvalidCustomCommand,
    #[error("custom status command cannot be enabled until a command is configured")]
    MissingCustomCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusCommandOutput {
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum StatusCommandError {
    #[error("custom status commands require a native sandbox")]
    SandboxUnavailable,
    #[error("custom status command could not be started")]
    StartFailed,
    #[error("custom status command timed out")]
    TimedOut,
    #[error("custom status command failed")]
    Failed,
    #[error("custom status command returned no text")]
    Empty,
    #[error("custom status command returned invalid UTF-8")]
    InvalidOutput,
}

#[derive(Debug, Clone)]
pub struct StatusCommandRunner {
    backend: crate::sandbox::SandboxBackend,
    enforce_network_denial: bool,
}

impl Default for StatusCommandRunner {
    fn default() -> Self {
        Self {
            backend: crate::sandbox::SandboxBackend::new(crate::sandbox::SandboxMode::Required),
            enforce_network_denial: true,
        }
    }
}

impl StatusCommandRunner {
    #[cfg(test)]
    fn with_backend(backend: crate::sandbox::SandboxBackend, enforce_network_denial: bool) -> Self {
        Self {
            backend,
            enforce_network_denial,
        }
    }

    pub async fn run(
        &self,
        command_text: &str,
        cwd: &Path,
        roots: &[PathBuf],
    ) -> Result<StatusCommandOutput, StatusCommandError> {
        if !self.enforce_network_denial {
            return Err(StatusCommandError::SandboxUnavailable);
        }
        let temporary_directory =
            std::env::temp_dir().join(format!("ca-status-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&temporary_directory).map_err(|_| StatusCommandError::StartFailed)?;
        let prepared = self
            .backend
            .prepare_shell(
                command_text,
                &crate::sandbox::SandboxPolicy {
                    roots: roots.to_vec(),
                    read_only_roots: Vec::new(),
                    temporary_directory: temporary_directory.clone(),
                    network_allowed: false,
                    hide_other_processes: false,
                },
            )
            .map_err(|_| StatusCommandError::SandboxUnavailable);
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&temporary_directory);
                return Err(error);
            }
        };

        let mut process = tokio::process::Command::new(prepared.program);
        process
            .args(prepared.arguments)
            .current_dir(cwd)
            .env_clear()
            .env(
                "PATH",
                "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin",
            )
            .env("TMPDIR", &temporary_directory)
            .env("CA_PROJECT_DIR", cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            process.as_std_mut().process_group(0);
        }
        let mut child = match process.spawn() {
            Ok(child) => child,
            Err(_) => {
                let _ = std::fs::remove_dir_all(&temporary_directory);
                return Err(StatusCommandError::StartFailed);
            }
        };
        let pid = child.id();
        let mut guard = StatusProcessGuard::new(pid, temporary_directory);
        let stdout_reader = tokio::spawn(read_bounded(
            child.stdout.take(),
            STATUS_COMMAND_CONTENT_BYTES,
        ));
        let stderr_reader = tokio::spawn(read_bounded(child.stderr.take(), 0));
        let status = tokio::time::timeout(STATUS_COMMAND_TIMEOUT, child.wait()).await;
        guard.terminate();
        if status.is_err() {
            let _ = child.start_kill();
        }
        let _ = child.wait().await;
        let stdout = stdout_reader.await.unwrap_or_default();
        let _ = stderr_reader.await;
        guard.complete();
        match status {
            Err(_) => return Err(StatusCommandError::TimedOut),
            Ok(Err(_)) => return Err(StatusCommandError::Failed),
            Ok(Ok(status)) if !status.success() => return Err(StatusCommandError::Failed),
            Ok(Ok(_)) => (),
        }
        let (source, retained_bytes) = valid_utf8_prefix(&stdout.kept)?;
        let safe = crate::terminal_text::sanitize_terminal_text(source);
        let one_line = safe.split_whitespace().collect::<Vec<_>>().join(" ");
        if one_line.is_empty() {
            return Err(StatusCommandError::Empty);
        }
        let omitted = stdout.total.saturating_sub(retained_bytes);
        let text = if omitted == 0 {
            one_line
        } else {
            format!("{one_line}… [{omitted} bytes omitted]")
        };
        Ok(StatusCommandOutput {
            truncated: omitted > 0,
            text,
        })
    }
}

#[derive(Default)]
struct BoundedRead {
    kept: Vec<u8>,
    total: usize,
}

async fn read_bounded<R>(reader: Option<R>, limit: usize) -> BoundedRead
where
    R: tokio::io::AsyncRead + Unpin,
{
    let Some(mut reader) = reader else {
        return BoundedRead::default();
    };
    let mut result = BoundedRead::default();
    let mut buffer = [0_u8; 1024];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                result.total = result.total.saturating_add(read);
                let remaining = limit.saturating_sub(result.kept.len());
                result
                    .kept
                    .extend_from_slice(&buffer[..read.min(remaining)]);
            }
        }
    }
    result
}

fn valid_utf8_prefix(bytes: &[u8]) -> Result<(&str, usize), StatusCommandError> {
    match std::str::from_utf8(bytes) {
        Ok(value) => Ok((value, bytes.len())),
        Err(error) if error.error_len().is_none() => {
            let end = error.valid_up_to();
            std::str::from_utf8(&bytes[..end])
                .map(|value| (value, end))
                .map_err(|_| StatusCommandError::InvalidOutput)
        }
        Err(_) => Err(StatusCommandError::InvalidOutput),
    }
}

fn terminate_process_group(pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(pid as i32),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
}

struct StatusProcessGuard {
    pid: Option<u32>,
    temporary_directory: PathBuf,
    armed: bool,
}

impl StatusProcessGuard {
    fn new(pid: Option<u32>, temporary_directory: PathBuf) -> Self {
        Self {
            pid,
            temporary_directory,
            armed: true,
        }
    }

    fn terminate(&self) {
        terminate_process_group(self.pid);
    }

    fn complete(&mut self) {
        self.armed = false;
        let _ = std::fs::remove_dir_all(&self.temporary_directory);
    }
}

impl Drop for StatusProcessGuard {
    fn drop(&mut self) {
        if self.armed {
            self.terminate();
            let _ = std::fs::remove_dir_all(&self.temporary_directory);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn defaults_preserve_existing_status_surface() {
        let config = UiConfig::default();
        assert_eq!(config.theme, UiTheme::Default);
        assert_eq!(config.notification, NotificationStyle::Off);
        assert!(!config.status_indicators);
        assert_eq!(config.mouse, MouseMode::Scroll);
        assert_eq!(config.statusline.fields, default_status_fields());
        assert!(!config.statusline.custom_enabled);
        assert!(!config.transcript.timestamps);
        assert!(!config.transcript.assistant_footers);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn transcript_metadata_and_usage_status_fields_are_explicit_and_default_off() {
        let config: UiConfig = toml::from_str(
            r#"
[transcript]
timestamps = true
assistant_footers = true

[statusline]
fields = ["generation_speed", "sitting_cost"]
"#,
        )
        .unwrap();
        assert!(config.transcript.timestamps);
        assert!(config.transcript.assistant_footers);
        assert_eq!(
            config.statusline.fields,
            vec![StatusField::GenerationSpeed, StatusField::SittingCost]
        );
        assert!(config.validate().is_ok());
    }

    #[test]
    fn notification_config_and_terminal_sequences_are_exact() {
        let config: UiConfig = toml::from_str("notification = \"osc9\"\n").unwrap();
        assert_eq!(config.notification, NotificationStyle::Osc9);
        assert_eq!(
            notification_sequence(NotificationStyle::Bel, false).as_deref(),
            Some("\u{7}")
        );
        let plain = "\u{1b}]9;ca: ready\u{7}";
        assert_eq!(
            notification_sequence(NotificationStyle::Osc9, false).as_deref(),
            Some(plain)
        );
        assert_eq!(
            notification_sequence(NotificationStyle::Osc9, true),
            Some(format!("\u{1b}Ptmux;\u{1b}{plain}\u{1b}\\"))
        );
        assert_eq!(notification_sequence(NotificationStyle::Off, false), None);
    }

    #[test]
    fn mouse_mode_defaults_to_scroll_and_accepts_explicit_off() {
        let defaulted: UiConfig = toml::from_str("").unwrap();
        assert_eq!(defaulted.mouse, MouseMode::Scroll);
        let disabled: UiConfig = toml::from_str("mouse = \"off\"\n").unwrap();
        assert_eq!(disabled.mouse, MouseMode::Off);
    }

    #[test]
    fn validation_rejects_duplicates_missing_commands_and_bad_intervals() {
        let mut config = UiConfig::default();
        config.statusline.fields.push(StatusField::Messages);
        assert_eq!(config.validate(), Err(UiConfigError::DuplicateField));
        config.statusline.fields.pop();
        config.statusline.custom_enabled = true;
        assert_eq!(config.validate(), Err(UiConfigError::MissingCustomCommand));
        config.statusline.custom_enabled = false;
        config.statusline.refresh_secs = 0;
        assert_eq!(
            config.validate(),
            Err(UiConfigError::InvalidRefreshInterval)
        );
    }

    #[tokio::test]
    async fn runner_requires_an_enforceable_sandbox() {
        let runner = StatusCommandRunner::with_backend(
            crate::sandbox::SandboxBackend::new(crate::sandbox::SandboxMode::PolicyOnly),
            false,
        );
        let output = runner.run("printf unsafe", Path::new("/tmp"), &[]).await;
        assert_eq!(output, Err(StatusCommandError::SandboxUnavailable));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn fake_required_sandbox_proves_profile_network_denial_and_output_bound() {
        let temp = tempfile::tempdir().unwrap();
        let fake = temp.path().join("sandbox-exec");
        std::fs::write(
            &fake,
            "#!/bin/sh\nprofile=$2\ncase \"$profile\" in *'(deny network*)'*) ;; *) exit 9 ;; esac\nshift 2\nexec \"$@\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let backend = crate::sandbox::SandboxBackend::with_executable(
            crate::sandbox::SandboxMode::Required,
            fake,
        );
        let runner = StatusCommandRunner::with_backend(backend, true);
        let result = runner
            .run(
                "printf 'ready\\n'; i=0; while [ $i -lt 5000 ]; do printf x; i=$((i+1)); done",
                temp.path(),
                &[temp.path().to_path_buf()],
            )
            .await
            .unwrap();
        assert!(result.truncated);
        assert!(result.text.starts_with("ready "));
        assert!(result.text.contains("bytes omitted"));
        assert!(result.text.len() <= STATUS_COMMAND_OUTPUT_BYTES);
    }

    #[tokio::test]
    async fn aborting_runner_kills_the_full_process_group() {
        let temp = tempfile::tempdir().unwrap();
        let fake = temp.path().join("sandbox-exec");
        std::fs::write(&fake, "#!/bin/sh\nshift 2\nexec \"$@\"\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let runner = StatusCommandRunner::with_backend(
            crate::sandbox::SandboxBackend::with_executable(
                crate::sandbox::SandboxMode::Required,
                fake,
            ),
            true,
        );
        let marker = temp.path().join("descendant-survived");
        let command = format!("(sleep 1; printf leaked > '{}') & wait", marker.display());
        let cwd = temp.path().to_path_buf();
        let root = cwd.clone();
        let task = tokio::spawn(async move { runner.run(&command, &cwd, &[root]).await });
        tokio::time::sleep(Duration::from_millis(100)).await;
        task.abort();
        let _ = task.await;
        tokio::time::sleep(Duration::from_millis(1_100)).await;
        assert!(!marker.exists());
    }
}
