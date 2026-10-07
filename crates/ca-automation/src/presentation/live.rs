//! Live JSONL stream rendering for Codex role output.
//!
//! Converts raw JSONL event streams into short, human-friendly terminal lines.
//! The raw logs remain the full machine record; this module is purely
//! presentation.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::thread;
use std::time::Instant;

use serde_json::Value;

use crate::config::LiveOutputStyle;

use super::formatting::{
    sentence_case, short_path, strip_ansi, truncate_center, truncate_inline, unquote_shell_arg,
};
use super::theme::{frame_width, Theme};

// ---------------------------------------------------------------------------
// Public stream renderers
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_jsonl_stream<R>(
    reader: R,
    path: PathBuf,
    role_label: String,
    raw_log_label: String,
    preview_lines: usize,
    preview_chars: usize,
    verbose_success: bool,
    theme: Theme,
    style: LiveOutputStyle,
) -> thread::JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut file = match File::create(&path) {
            Ok(file) => file,
            Err(_) => return,
        };
        let mut renderer = LiveRenderer::new(
            role_label,
            raw_log_label,
            preview_lines,
            preview_chars,
            verbose_success,
            theme,
            style,
        );
        let reader = BufReader::new(reader);
        for line in reader.lines() {
            let Ok(line) = line else {
                break;
            };
            let _ = writeln!(file, "{line}");
            renderer.render_line(&line);
        }
    })
}

pub(crate) fn render_stderr_stream<R>(
    reader: R,
    path: PathBuf,
    role_label: String,
    theme: Theme,
    style: LiveOutputStyle,
) -> thread::JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut file = match File::create(&path) {
            Ok(file) => file,
            Err(_) => return,
        };
        let mut suppress_codex_tool_details = 0usize;
        let reader = BufReader::new(reader);
        for line in reader.lines() {
            let Ok(line) = line else {
                break;
            };
            let _ = writeln!(file, "{line}");
            let trimmed = line.trim();
            if !trimmed.is_empty() && !is_non_actionable_stderr(trimmed) {
                if let Some(summary) = summarize_codex_tool_stderr(trimmed) {
                    match style {
                        LiveOutputStyle::Boxed => {
                            eprintln!("{} {}", theme.tag(&role_label), theme.warning(&summary))
                        }
                        LiveOutputStyle::Codex => {
                            eprintln!("{}", theme.warning(&sentence_case(&summary)))
                        }
                    }
                    suppress_codex_tool_details = 80;
                    continue;
                }
                if suppress_codex_tool_details > 0 {
                    if is_codex_tool_stderr_detail(trimmed) || !trimmed.contains("codex_core::") {
                        suppress_codex_tool_details -= 1;
                        continue;
                    }
                    suppress_codex_tool_details = 0;
                }
                match style {
                    LiveOutputStyle::Boxed => eprintln!(
                        "{} {}",
                        theme.tag(&format!("{role_label} stderr")),
                        theme.error(&truncate_inline(trimmed, 240))
                    ),
                    LiveOutputStyle::Codex => {
                        eprintln!("{}", theme.error(&truncate_inline(trimmed, 240)))
                    }
                }
            }
        }
    })
}

// ---------------------------------------------------------------------------
// LiveRenderer
// ---------------------------------------------------------------------------

struct LiveRenderer {
    role_label: String,
    raw_log_label: String,
    preview_lines: usize,
    preview_chars: usize,
    verbose_success: bool,
    theme: Theme,
    style: LiveOutputStyle,
    command_starts: HashMap<String, Instant>,
    command_printed: HashMap<String, bool>,
    last_git_status_output: Option<String>,
    last_file_change_summary: Option<String>,
    swiftpm_sandbox_failures: HashSet<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandOutputAction {
    Show,
    DuplicateGitStatus,
    Hide,
}

impl LiveRenderer {
    fn new(
        role_label: String,
        raw_log_label: String,
        preview_lines: usize,
        preview_chars: usize,
        verbose_success: bool,
        theme: Theme,
        style: LiveOutputStyle,
    ) -> Self {
        Self {
            role_label,
            raw_log_label,
            preview_lines,
            preview_chars,
            verbose_success,
            theme,
            style,
            command_starts: HashMap::new(),
            command_printed: HashMap::new(),
            last_git_status_output: None,
            last_file_change_summary: None,
            swiftpm_sandbox_failures: HashSet::new(),
        }
    }

    fn render_line(&mut self, line: &str) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            println!(
                "{} {}",
                self.theme.tag(&self.role_label),
                truncate_inline(line.trim(), self.preview_chars)
            );
            return;
        };

        let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");
        match event_type {
            "turn.started" => {
                if self.style == LiveOutputStyle::Boxed {
                    println!(
                        "{} {}",
                        self.theme.tag(&self.role_label),
                        self.theme.dim("turn started")
                    );
                }
            }
            "turn.completed" => {
                if self.style == LiveOutputStyle::Boxed {
                    println!(
                        "{} {}",
                        self.theme.tag(&self.role_label),
                        self.theme.dim("turn completed")
                    );
                }
            }
            "item.started" => self.render_item_started(&value),
            "item.completed" => self.render_item_completed(&value),
            "error" => {
                let message = value
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown Codex error");
                println!(
                    "{} {}",
                    self.theme.tag(&self.role_label),
                    self.theme
                        .error(&format!("ERROR: {}", truncate_inline(message, 500)))
                );
            }
            _ => self.render_text_event(&value),
        }
    }

    fn render_item_started(&mut self, value: &Value) {
        let Some(item) = value.get("item") else {
            return;
        };
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
        if item_type == "command_execution" {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                self.command_starts.insert(id.to_string(), Instant::now());
            }
            if let Some(command) = item.get("command").and_then(Value::as_str) {
                let command = friendly_command(command);
                let should_print = self.verbose_success || is_live_start_command(&command);
                if let Some(id) = item.get("id").and_then(Value::as_str) {
                    self.command_printed.insert(id.to_string(), should_print);
                }
                if should_print {
                    self.render_command_line(&command);
                }
            }
            return;
        }

        if item_type == "file_change" && self.style == LiveOutputStyle::Codex {
            self.render_file_change_item(item);
        }
    }

    fn render_item_completed(&mut self, value: &Value) {
        let Some(item) = value.get("item") else {
            return;
        };
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
        if item_type == "command_execution" {
            let id = item.get("id").and_then(Value::as_str).unwrap_or("");
            let command = item
                .get("command")
                .and_then(Value::as_str)
                .map(friendly_command)
                .unwrap_or_default();
            let printed_at_start = self.command_printed.remove(id).unwrap_or(false);
            let elapsed = self
                .command_starts
                .remove(id)
                .map(|started| {
                    format!(
                        " in {}",
                        super::formatting::format_duration(started.elapsed())
                    )
                })
                .unwrap_or_default();
            let exit_code = item.get("exit_code").and_then(Value::as_i64);
            let status = match exit_code {
                Some(0) => "PASS",
                Some(_) => "FAIL",
                None => item
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("completed"),
            };
            let output_action = item
                .get("aggregated_output")
                .and_then(Value::as_str)
                .map(|output| self.command_output_action(exit_code, &command, output))
                .unwrap_or(CommandOutputAction::Hide);
            if self.style == LiveOutputStyle::Codex {
                let output = item.get("aggregated_output").and_then(Value::as_str);
                self.render_codex_command_completed(
                    &command,
                    exit_code,
                    &elapsed,
                    output_action,
                    output,
                );
                return;
            }

            let should_expand = exit_code != Some(0)
                || self.verbose_success
                || is_live_start_command(&command)
                || output_action == CommandOutputAction::Show;
            if should_expand {
                if !printed_at_start {
                    self.render_command_line(&command);
                }
                self.render_command_status(status, exit_code, &elapsed);
            } else {
                self.render_compact_command_success(&command, &elapsed);
            }
            if let Some(output) = item.get("aggregated_output").and_then(Value::as_str) {
                match output_action {
                    CommandOutputAction::Show => self.render_output_preview(output),
                    CommandOutputAction::DuplicateGitStatus => {
                        println!(
                            "{} {}",
                            self.theme.tag(&self.role_label),
                            self.theme.dim("same git status as previous check")
                        );
                    }
                    CommandOutputAction::Hide => {}
                }
            }
            return;
        }

        if item_type == "file_change" {
            if self.style == LiveOutputStyle::Codex {
                self.render_file_change_item(item);
            } else {
                self.render_text_event(item);
            }
            return;
        }

        if item_type == "agent_message" {
            self.render_agent_message(item);
            return;
        }

        self.render_text_event(item);
    }

    fn render_command_line(&self, command: &str) {
        if self.style == LiveOutputStyle::Codex {
            println!(
                "{}",
                self.theme
                    .dim(&format!("Running {}", truncate_center(command, 96)))
            );
            return;
        }
        let max = frame_width().saturating_sub(self.role_label.chars().count() + 10);
        println!(
            "{} {} {}",
            self.theme.tag(&self.role_label),
            self.theme.command("$"),
            self.theme.command(&truncate_center(command, max.max(40)))
        );
    }

    fn render_codex_command_completed(
        &mut self,
        command: &str,
        exit_code: Option<i64>,
        elapsed: &str,
        output_action: CommandOutputAction,
        output: Option<&str>,
    ) {
        if output_action == CommandOutputAction::DuplicateGitStatus {
            println!("{}", self.theme.dim("Checked git status (unchanged)"));
            return;
        }

        if is_rg_no_matches(command, exit_code) {
            println!("{}", self.theme.dim(&rg_no_matches_activity(command)));
            return;
        }

        if let Some(output) = output {
            if is_swiftpm_sandbox_failure(command, exit_code, output) {
                let key = swiftpm_command_family(command);
                let first_seen = self.swiftpm_sandbox_failures.insert(key.clone());
                let note = if first_seen {
                    format!(
                        "SwiftPM sandbox blocked {}; raw log has manifest/cache details",
                        truncate_center(&key, 80)
                    )
                } else {
                    format!(
                        "SwiftPM sandbox blocked {} again; raw log has details",
                        truncate_center(&key, 80)
                    )
                };
                println!("{}", self.theme.warning(&note));
                return;
            }
        }

        let activity = codex_command_activity(command, exit_code, elapsed);
        if exit_code == Some(0) {
            println!("{}", self.theme.dim(&activity));
        } else {
            println!("{}", self.theme.error(&activity));
            if let Some(output) = output {
                self.render_output_preview(output);
            }
        }
    }

    fn render_file_change_item(&mut self, item: &Value) {
        let Some(changes) = item.get("changes").and_then(Value::as_array) else {
            return;
        };
        if changes.is_empty() {
            return;
        }

        let mut paths = changes
            .iter()
            .filter_map(|change| change.get("path").and_then(Value::as_str))
            .map(short_path)
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();

        let summary = if paths.len() == 1 {
            format!("Edited {}", paths[0])
        } else {
            let shown = paths.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            let suffix = if paths.len() > 3 {
                format!(", and {} more", paths.len() - 3)
            } else {
                String::new()
            };
            format!("Edited {} files: {shown}{suffix}", paths.len())
        };

        if self.last_file_change_summary.as_deref() == Some(summary.as_str()) {
            return;
        }
        self.last_file_change_summary = Some(summary.clone());
        println!("{}", self.theme.dim(&summary));
    }

    fn render_command_status(&self, status: &str, exit_code: Option<i64>, elapsed: &str) {
        match exit_code {
            Some(code) => println!(
                "{} {} {}",
                self.theme.tag(&self.role_label),
                self.theme.status(status, code == 0),
                self.theme.dim(&format!("exit {code}{elapsed}"))
            ),
            None => println!(
                "{} {}",
                self.theme.tag(&self.role_label),
                self.theme.dim(&format!("{status}{elapsed}"))
            ),
        }
    }

    fn render_compact_command_success(&self, command: &str, elapsed: &str) {
        let max = frame_width().saturating_sub(self.role_label.chars().count() + 18);
        println!(
            "{} {} {} {}",
            self.theme.tag(&self.role_label),
            self.theme.status("ok", true),
            self.theme.dim(elapsed.trim_start()),
            self.theme.dim(&truncate_center(command, max.max(32)))
        );
    }

    fn command_output_action(
        &mut self,
        exit_code: Option<i64>,
        command: &str,
        output: &str,
    ) -> CommandOutputAction {
        let output = output.trim();
        if output.is_empty() {
            return CommandOutputAction::Hide;
        }
        if is_rg_no_matches(command, exit_code)
            || is_swiftpm_sandbox_failure(command, exit_code, output)
        {
            return CommandOutputAction::Hide;
        }
        if exit_code != Some(0) {
            return CommandOutputAction::Show;
        }
        if self.verbose_success {
            return CommandOutputAction::Show;
        }
        if command.trim() == "git status --short" {
            let normalized = output.to_string();
            let duplicate = self
                .last_git_status_output
                .as_ref()
                .map(|last| last == &normalized)
                .unwrap_or(false);
            self.last_git_status_output = Some(normalized);
            return if duplicate {
                CommandOutputAction::DuplicateGitStatus
            } else {
                CommandOutputAction::Show
            };
        }
        if is_high_signal_success_output(command) {
            CommandOutputAction::Show
        } else {
            CommandOutputAction::Hide
        }
    }

    fn render_agent_message(&self, item: &Value) {
        let Some(text) = item.get("text").and_then(Value::as_str) else {
            self.render_text_event(item);
            return;
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            self.render_structured_agent_result(&value);
        } else {
            self.render_paragraph(trimmed);
        }
    }

    fn render_structured_agent_result(&self, value: &Value) {
        if let Some(task_id) = value.get("task_id").and_then(Value::as_str) {
            let status = value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("completed");
            if self.style == LiveOutputStyle::Codex {
                println!(
                    "\n{}",
                    self.theme
                        .status(&format!("{task_id} -> {status}"), status == "done")
                );
            } else {
                println!(
                    "{} {} {}",
                    self.theme.tag(&self.role_label),
                    self.theme.label("RESULT"),
                    self.theme
                        .status(&format!("{task_id} -> {status}"), status == "done")
                );
            }
            if let Some(summary) = value.get("summary").and_then(Value::as_str) {
                self.render_labeled_or_paragraph("SUMMARY", summary);
            }
            if let Some(blocker) = value.get("blocker").and_then(Value::as_str) {
                if !blocker.trim().is_empty() {
                    if self.style == LiveOutputStyle::Codex {
                        self.render_paragraph(&format!(
                            "Blocked: {}",
                            truncate_inline(blocker, 500)
                        ));
                    } else {
                        println!(
                            "{} {} {}",
                            self.theme.tag(&self.role_label),
                            self.theme.label("BLOCKER"),
                            self.theme.error(&truncate_inline(blocker, 500))
                        );
                    }
                }
            }
            return;
        }

        if value.get("accepted").is_some() {
            let accepted = value
                .get("accepted")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let commit = value
                .get("commit_sha")
                .and_then(Value::as_str)
                .filter(|sha| !sha.is_empty())
                .map(|sha| format!("; commit {}", truncate_inline(sha, 12)))
                .unwrap_or_default();
            if self.style == LiveOutputStyle::Codex {
                let label = if accepted {
                    "Review accepted"
                } else {
                    "Review rejected"
                };
                println!(
                    "\n{}{}",
                    self.theme.status(label, accepted),
                    self.theme.dim(&commit)
                );
            } else {
                println!(
                    "{} {} {}{}",
                    self.theme.tag(&self.role_label),
                    self.theme.label("RESULT"),
                    self.theme.status(
                        if accepted {
                            "review accepted"
                        } else {
                            "review rejected"
                        },
                        accepted
                    ),
                    self.theme.dim(&commit)
                );
            }
            if let Some(summary) = value.get("summary").and_then(Value::as_str) {
                self.render_labeled_or_paragraph("SUMMARY", summary);
            }
            let replacements = value
                .get("replacement_tasks")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            let followups = value
                .get("followup_tasks")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            if replacements > 0 || followups > 0 {
                if self.style == LiveOutputStyle::Codex {
                    println!(
                        "{}",
                        self.theme.dim(&format!(
                            "Queued {replacements} replacement(s), {followups} follow-up(s)"
                        ))
                    );
                } else {
                    println!(
                        "{} {} {} replacement(s), {} follow-up(s)",
                        self.theme.tag(&self.role_label),
                        self.theme.label("QUEUE"),
                        replacements,
                        followups
                    );
                }
            }
            return;
        }

        if let Some(summary) = value.get("summary").and_then(Value::as_str) {
            self.render_labeled_or_paragraph("SUMMARY", summary);
        }
    }

    fn render_text_event(&self, value: &Value) {
        if let Some(text) = extract_event_text(value) {
            let text = text.trim();
            if !text.is_empty() {
                if is_codex_config_warning(text) {
                    if self.style == LiveOutputStyle::Codex {
                        println!(
                            "{}",
                            self.theme
                                .warning(&format!("Warning: {}", truncate_inline(text, 500)))
                        );
                    } else {
                        println!(
                            "{} {}",
                            self.theme.tag("Codex Config"),
                            self.theme
                                .warning(&format!("WARNING: {}", truncate_inline(text, 500)))
                        );
                    }
                } else {
                    self.render_paragraph(text);
                }
            }
        }
    }

    fn render_labeled_or_paragraph(&self, label: &str, text: &str) {
        if self.style == LiveOutputStyle::Codex {
            self.render_paragraph(text);
        } else {
            println!(
                "{} {} {}",
                self.theme.tag(&self.role_label),
                self.theme.label(label),
                truncate_inline(text, 700)
            );
        }
    }

    fn render_paragraph(&self, text: &str) {
        if self.style == LiveOutputStyle::Codex {
            println!("\n{}", truncate_inline(text, self.preview_chars.max(500)));
        } else {
            println!(
                "{} {}",
                self.theme.tag(&self.role_label),
                truncate_inline(text, 500)
            );
        }
    }

    fn render_output_preview(&self, output: &str) {
        let preview = preview_text(output, self.preview_lines, self.preview_chars);
        if preview.lines.is_empty() {
            return;
        }
        if self.style == LiveOutputStyle::Boxed {
            println!(
                "{} {}",
                self.theme.tag(&self.role_label),
                self.theme.label("OUTPUT")
            );
        }
        for line in preview.lines {
            if self.style == LiveOutputStyle::Codex {
                println!("{}", self.theme.dim(&format!("  {line}")));
            } else {
                println!("  {}", self.theme.dim(&line));
            }
        }
        if preview.truncated {
            println!(
                "{}",
                self.theme.dim(&format!(
                    "  ... truncated; full output in {}",
                    self.raw_log_label
                ))
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

pub(crate) struct Preview {
    pub(crate) lines: Vec<String>,
    pub(crate) truncated: bool,
}

pub(crate) fn preview_text(text: &str, max_lines: usize, max_chars: usize) -> Preview {
    let trimmed = text.trim();
    if trimmed.is_empty() || max_lines == 0 || max_chars == 0 {
        return Preview {
            lines: Vec::new(),
            truncated: false,
        };
    }

    let mut lines = Vec::new();
    let mut used_chars = 0;
    let mut truncated = false;
    for line in trimmed.lines() {
        if lines.len() >= max_lines || used_chars >= max_chars {
            truncated = true;
            break;
        }
        let remaining = max_chars.saturating_sub(used_chars);
        let clean = strip_ansi(line.trim_end());
        let visible = truncate_inline(&clean, remaining.min(240));
        used_chars += visible.chars().count() + 1;
        lines.push(visible);
    }

    if trimmed.lines().count() > lines.len() {
        truncated = true;
    }

    Preview { lines, truncated }
}

// ---------------------------------------------------------------------------
// Event text extraction
// ---------------------------------------------------------------------------

fn extract_event_text(value: &Value) -> Option<String> {
    for key in ["message", "text", "delta", "summary"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            return Some(text.to_string());
        }
    }

    let content = value.get("content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    if let Some(items) = content.as_array() {
        let mut parts = Vec::new();
        for item in items {
            if let Some(text) = item.get("text").and_then(Value::as_str) {
                parts.push(text);
            }
        }
        if !parts.is_empty() {
            return Some(parts.join("\n"));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Stderr classification helpers
// ---------------------------------------------------------------------------

fn is_codex_config_warning(text: &str) -> bool {
    text.starts_with("Ignoring malformed agent role definition")
}

pub(crate) fn is_non_actionable_stderr(text: &str) -> bool {
    text.contains("failed to record rollout items")
}

pub(crate) fn summarize_codex_tool_stderr(text: &str) -> Option<String> {
    if text.contains("codex_core::tools::router")
        && text.contains("apply_patch verification failed")
    {
        return Some("tool note: apply_patch context mismatch; raw stderr log has details".into());
    }
    if text.contains("codex_core::tools::router") && text.contains("exec_command failed") {
        return Some("tool note: command failed inside Codex; stream shows the command result when available".into());
    }
    None
}

pub(crate) fn is_codex_tool_stderr_detail(text: &str) -> bool {
    !text.contains("codex_core::")
        && !text.contains(" ERROR ")
        && (text.starts_with('-')
            || text.starts_with('+')
            || text.starts_with("@@")
            || text.starts_with('`')
            || text.starts_with("and ")
            || text.starts_with("Failed ")
            || text.starts_with("quickstart")
            || text.starts_with("sandbox")
            || text.starts_with("expected"))
}

// ---------------------------------------------------------------------------
// Command classification helpers
// ---------------------------------------------------------------------------

pub(crate) fn is_high_signal_success_output(command: &str) -> bool {
    let command = command.trim();
    command == "git status --short"
        || command == "git diff --stat"
        || command.starts_with("git commit")
        || command.contains(" git commit ")
        || is_common_build_test_command(command)
        || command.starts_with(".build/")
        || command.starts_with("./")
        || command.starts_with("tools/")
}

pub(crate) fn is_live_start_command(command: &str) -> bool {
    let command = command.trim();
    is_common_build_test_command(command)
        || command.starts_with(".build/")
        || command.starts_with("./")
        || command.starts_with("tools/")
        || command.contains("apply_patch")
        || command.starts_with("git add")
        || command.starts_with("git commit")
        || command.contains(" git commit ")
}

fn is_common_build_test_command(command: &str) -> bool {
    let command = command.trim();
    [
        "cargo build",
        "cargo check",
        "cargo clippy",
        "cargo test",
        "go build",
        "go test",
        "mvn ",
        "gradle ",
        "./gradlew ",
        "make ",
        "npm run",
        "npm test",
        "pnpm ",
        "yarn ",
        "pytest",
        "python -m pytest",
        "swift build",
        "swift run",
        "swift test",
    ]
    .iter()
    .any(|prefix| command == prefix.trim_end() || command.starts_with(prefix))
}

pub(crate) fn friendly_command(command: &str) -> String {
    for prefix in ["/bin/zsh -lc ", "zsh -lc ", "/bin/bash -lc ", "bash -lc "] {
        if let Some(rest) = command.strip_prefix(prefix) {
            return unquote_shell_arg(rest).to_string();
        }
    }
    command.to_string()
}

fn codex_command_activity(command: &str, exit_code: Option<i64>, elapsed: &str) -> String {
    let command = command.trim();
    let duration = elapsed
        .trim()
        .strip_prefix("in ")
        .unwrap_or_else(|| elapsed.trim());

    if exit_code != Some(0) {
        let code = exit_code
            .map(|code| format!(" exit {code}"))
            .unwrap_or_default();
        return format!(
            "Failed {}{}{}",
            truncate_center(command, 100),
            code,
            duration_suffix(duration)
        );
    }

    if let Some(read) = read_activity(command) {
        return read;
    }
    if let Some(search) = search_activity(command) {
        return search;
    }

    format!(
        "Ran {}{}",
        truncate_center(command, 110),
        duration_suffix(duration)
    )
}

pub(crate) fn is_rg_no_matches(command: &str, exit_code: Option<i64>) -> bool {
    exit_code == Some(1) && command.trim().starts_with("rg ")
}

pub(crate) fn rg_no_matches_activity(command: &str) -> String {
    if let Some(search) = search_activity(command) {
        return search.replacen("Searched for", "No matches for", 1);
    }
    format!("No matches from {}", truncate_center(command.trim(), 90))
}

pub(crate) fn is_swiftpm_sandbox_failure(
    command: &str,
    exit_code: Option<i64>,
    output: &str,
) -> bool {
    if exit_code == Some(0) || !is_swiftpm_command(command) {
        return false;
    }
    output.contains("sandbox-exec: sandbox_apply: Operation not permitted")
        || output.contains("/.cache/clang/ModuleCache")
            && output.contains("Operation not permitted")
        || output.contains("Invalid manifest")
            && output.contains("unable to load standard library")
            && output.contains("Operation not permitted")
}

pub(crate) fn is_swiftpm_command(command: &str) -> bool {
    command.split("&&").map(str::trim).any(|part| {
        part.starts_with("swift build")
            || part.starts_with("swift test")
            || part.starts_with("swift run")
            || part.contains(" swift build")
            || part.contains(" swift test")
            || part.contains(" swift run")
    })
}

fn swiftpm_command_family(command: &str) -> String {
    let mut families = command
        .split("&&")
        .map(str::trim)
        .filter(|part| part.contains("swift "))
        .map(|part| {
            if let Some(index) = part.find("swift ") {
                truncate_center(part[index..].trim(), 96)
            } else {
                truncate_center(part, 96)
            }
        })
        .collect::<Vec<_>>();
    families.sort();
    families.dedup();
    if families.is_empty() {
        "SwiftPM command".to_string()
    } else {
        families.join(" + ")
    }
}

fn duration_suffix(duration: &str) -> String {
    if duration.is_empty() {
        String::new()
    } else {
        format!(" for {duration}")
    }
}

fn read_activity(command: &str) -> Option<String> {
    let command = command.trim();
    let read_like = command.starts_with("sed ")
        || command.starts_with("cat ")
        || command.starts_with("tail ")
        || command.starts_with("head ");
    if !read_like {
        return None;
    }

    let files = command_file_tokens(command);
    Some(match files.len() {
        0 => format!("Read {}", truncate_center(command, 100)),
        1 => format!("Read {}", short_path(&files[0])),
        n => format!("Read {n} files"),
    })
}

fn search_activity(command: &str) -> Option<String> {
    let command = command.trim();
    if !command.starts_with("rg ") {
        return None;
    }

    let query = first_quoted_arg(command)
        .or_else(|| command.split_whitespace().nth(2).map(str::to_string))
        .unwrap_or_else(|| "pattern".to_string());
    Some(format!("Searched for {}", truncate_center(&query, 90)))
}

fn command_file_tokens(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .map(|token| token.trim_matches(|c| c == '\'' || c == '"' || c == ',' || c == ';'))
        .filter(|token| {
            !token.starts_with('-')
                && (token.contains('/')
                    || token.ends_with(".md")
                    || token.ends_with(".swift")
                    || token.ends_with(".toml")
                    || token.ends_with(".json")
                    || token.ends_with(".sh"))
        })
        .map(str::to_string)
        .collect()
}

fn first_quoted_arg(command: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let Some(start) = command.find(quote) else {
            continue;
        };
        let rest = &command[start + 1..];
        let Some(end) = rest.find(quote) else {
            continue;
        };
        if end > 0 {
            return Some(rest[..end].to_string());
        }
    }
    None
}
