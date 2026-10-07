use std::collections::BTreeMap;

const MAX_SUMMARY_BYTES: usize = 16 * 1024;
const MAX_LISTED_PATHS: usize = 12;
const MAX_LISTED_DIAGNOSTICS: usize = 20;
const HUMAN_SUMMARY_MARKER: &str =
    "[semantic presentation; transformed/omitted detail remains in the bounded raw Ctrl+O expansion]";
const MODEL_SUMMARY_MARKER: &str =
    "[semantic command summary; transformed/omitted detail remains in the process-local bounded raw capture]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    GitStatus,
    GitDiff,
    CargoTest,
    CargoClippy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandObservation {
    pub command: String,
    pub cwd: String,
    pub network_requested: bool,
    pub status: crate::jobs::JobStatus,
    pub stdout: String,
    pub stderr: String,
    pub stdout_omitted_bytes: usize,
    pub stderr_omitted_bytes: usize,
    pub bounded_model_text: String,
}

impl CommandObservation {
    pub fn from_job(job: &crate::jobs::JobInfo, network_requested: bool) -> Self {
        Self {
            command: job.command.clone(),
            cwd: job.cwd.clone(),
            network_requested,
            status: job.status,
            stdout: job.stdout.clone(),
            stderr: job.stderr.clone(),
            stdout_omitted_bytes: job.stdout_omitted_bytes,
            stderr_omitted_bytes: job.stderr_omitted_bytes,
            bounded_model_text: job.formatted_output(),
        }
    }

    fn capture_incomplete(&self) -> bool {
        self.stdout_omitted_bytes > 0 || self.stderr_omitted_bytes > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullOutputRerun {
    pub command: String,
    pub cwd: String,
    pub network_requested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticPresentation {
    pub kind: CommandKind,
    pub collapsed: String,
    pub raw: String,
    pub rerun: FullOutputRerun,
}

pub fn classify_command(command: &str) -> Option<CommandKind> {
    let words = closed_shell_words(command)?;
    let (program, subcommand, rest) = match words.as_slice() {
        [program, subcommand, rest @ ..] => (program.as_str(), subcommand.as_str(), rest),
        _ => return None,
    };
    if rest.iter().any(|argument| {
        matches!(argument.as_str(), "-h" | "--help" | "--version")
            || argument.starts_with("--message-format")
    }) {
        return None;
    }
    match (program, subcommand) {
        ("git", "status")
            if !rest
                .iter()
                .any(|argument| argument.starts_with("--porcelain=v2")) =>
        {
            Some(CommandKind::GitStatus)
        }
        ("git", "diff")
            if !rest.iter().any(|argument| {
                matches!(
                    argument.as_str(),
                    "--no-index" | "--ext-diff" | "--textconv" | "--quiet"
                ) || argument == "--output"
                    || argument.starts_with("--output=")
            }) =>
        {
            Some(CommandKind::GitDiff)
        }
        ("cargo", "test") => Some(CommandKind::CargoTest),
        ("cargo", "clippy") => Some(CommandKind::CargoClippy),
        _ => None,
    }
}

pub fn summarize(observation: &CommandObservation) -> Option<SemanticPresentation> {
    let kind = classify_command(&observation.command)?;
    let mut collapsed = match kind {
        CommandKind::GitStatus => summarize_git_status(observation)?,
        CommandKind::GitDiff => summarize_git_diff(observation)?,
        CommandKind::CargoTest => summarize_cargo_test(observation)?,
        CommandKind::CargoClippy => summarize_cargo_clippy(observation)?,
    };
    if observation.capture_incomplete() {
        collapsed.push_str(&format!(
            "\n[source capture incomplete: {} stdout bytes and {} stderr bytes omitted before semantic processing]",
            observation.stdout_omitted_bytes, observation.stderr_omitted_bytes
        ));
    }
    collapsed.push('\n');
    collapsed.push_str(HUMAN_SUMMARY_MARKER);
    let collapsed = bound_summary(collapsed);
    Some(SemanticPresentation {
        kind,
        collapsed,
        raw: observation.bounded_model_text.clone(),
        rerun: FullOutputRerun {
            command: observation.command.clone(),
            cwd: observation.cwd.clone(),
            network_requested: observation.network_requested,
        },
    })
}

pub fn model_text_for_policy(
    result: &crate::agent::registry::ToolExecutionResult,
    policy: crate::config::CommandOutputPolicy,
) -> String {
    if policy == crate::config::CommandOutputPolicy::Semantic {
        if let Some(mut summary) = result.command_observation().and_then(summarize) {
            summary.collapsed = summary
                .collapsed
                .replace(HUMAN_SUMMARY_MARKER, MODEL_SUMMARY_MARKER);
            return summary.collapsed;
        }
    }
    result.model_text().to_string()
}

fn closed_shell_words(command: &str) -> Option<Vec<String>> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }

    if command.is_empty()
        || command
            .chars()
            .any(|character| matches!(character, '\0' | '\n' | '\r'))
    {
        return None;
    }
    let mut words = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote = Quote::None;
    let mut characters = command.chars();
    while let Some(character) = characters.next() {
        match quote {
            Quote::None if character.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut current));
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
            Quote::None if character == '\\' => {
                let escaped = characters.next()?;
                if escaped.is_control() {
                    return None;
                }
                current.push(escaped);
                started = true;
            }
            Quote::None
                if matches!(
                    character,
                    ';' | '|'
                        | '&'
                        | '<'
                        | '>'
                        | '$'
                        | '`'
                        | '('
                        | ')'
                        | '{'
                        | '}'
                        | '['
                        | ']'
                        | '*'
                        | '?'
                        | '~'
                        | '#'
                ) =>
            {
                return None;
            }
            Quote::None => {
                if character.is_control() {
                    return None;
                }
                current.push(character);
                started = true;
            }
            Quote::Single if character == '\'' => quote = Quote::None,
            Quote::Single => {
                if character.is_control() {
                    return None;
                }
                current.push(character);
            }
            Quote::Double if character == '"' => quote = Quote::None,
            Quote::Double if matches!(character, '$' | '`') => return None,
            Quote::Double if character == '\\' => {
                let escaped = characters.next()?;
                if escaped.is_control() {
                    return None;
                }
                current.push(escaped);
            }
            Quote::Double => {
                if character.is_control() {
                    return None;
                }
                current.push(character);
            }
        }
    }
    if quote != Quote::None {
        return None;
    }
    if started {
        words.push(current);
    }
    (!words.is_empty()).then_some(words)
}

fn summarize_git_status(observation: &CommandObservation) -> Option<String> {
    let output = observation.stdout.trim_end_matches('\n');
    if output.is_empty() {
        return Some(format!(
            "Git status · clean · {}",
            terminal_status(observation.status)
        ));
    }
    let mut groups = BTreeMap::<&'static str, Vec<String>>::new();
    let mut long_section = None;
    let mut parsed = 0_usize;
    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.len() >= 3 {
            let bytes = line.as_bytes();
            let code = &line[..2];
            if bytes[2] == b' ' && is_short_status_code(code) {
                let label = short_status_label(code);
                groups
                    .entry(label)
                    .or_default()
                    .push(safe(line[3..].trim()));
                parsed += 1;
                continue;
            }
        }
        let trimmed = line.trim();
        if let Some((label, path)) = long_status_entry(trimmed) {
            groups.entry(label).or_default().push(safe(path));
            parsed += 1;
            continue;
        }
        if trimmed == "Untracked files:" {
            long_section = Some("untracked");
            continue;
        }
        if trimmed.ends_with(':')
            || trimmed.starts_with("On branch ")
            || trimmed.starts_with("Your branch ")
            || trimmed.starts_with("Your branch is ")
            || trimmed.starts_with("Changes ")
            || trimmed.starts_with("Unmerged paths")
            || trimmed.starts_with("no changes added")
            || trimmed == "nothing to commit, working tree clean"
            || trimmed.starts_with("(use ")
        {
            if trimmed != "Untracked files:" {
                long_section = None;
            }
            continue;
        }
        if line.starts_with('\t') {
            if let Some(label) = long_section {
                groups.entry(label).or_default().push(safe(trimmed));
                parsed += 1;
                continue;
            }
        }
        return None;
    }
    if parsed == 0 {
        return None;
    }
    let total = groups.values().map(Vec::len).sum::<usize>();
    let mut result = format!(
        "Git status · {total} {} · {}",
        if total == 1 { "path" } else { "paths" },
        terminal_status(observation.status)
    );
    let counts = groups
        .iter()
        .map(|(label, paths)| format!("{label} {}", paths.len()))
        .collect::<Vec<_>>();
    result.push('\n');
    result.push_str(&counts.join(" · "));
    for (label, paths) in groups {
        result.push_str(&format!("\n{label}:"));
        for path in paths.iter().take(MAX_LISTED_PATHS) {
            result.push_str(&format!("\n  {path}"));
        }
        if paths.len() > MAX_LISTED_PATHS {
            result.push_str(&format!(
                "\n  +{} paths omitted from semantic summary",
                paths.len() - MAX_LISTED_PATHS
            ));
        }
    }
    Some(result)
}

fn is_short_status_code(code: &str) -> bool {
    code == "??"
        || code == "!!"
        || code
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'M' | b'T' | b'A' | b'D' | b'R' | b'C' | b'U'))
}

fn short_status_label(code: &str) -> &'static str {
    if code == "??" {
        "untracked"
    } else if code == "!!" {
        "ignored"
    } else if code.contains('U') || matches!(code, "AA" | "DD") {
        "conflicted"
    } else if code.contains('R') {
        "renamed"
    } else if code.contains('C') {
        "copied"
    } else if code.contains('D') {
        "deleted"
    } else if code.contains('A') {
        "added"
    } else if code.contains('T') {
        "type-changed"
    } else {
        "modified"
    }
}

fn long_status_entry(line: &str) -> Option<(&'static str, &str)> {
    for (prefix, label) in [
        ("modified:", "modified"),
        ("new file:", "added"),
        ("deleted:", "deleted"),
        ("renamed:", "renamed"),
        ("copied:", "copied"),
        ("both modified:", "conflicted"),
        ("both added:", "conflicted"),
    ] {
        if let Some(path) = line.strip_prefix(prefix).map(str::trim) {
            if !path.is_empty() {
                return Some((label, path));
            }
        }
    }
    None
}

#[derive(Default)]
struct DiffFile {
    path: String,
    added: usize,
    deleted: usize,
    binary: bool,
}

fn summarize_git_diff(observation: &CommandObservation) -> Option<String> {
    if observation.stdout.is_empty() {
        return Some(format!(
            "Git diff · no captured changes · {}",
            terminal_status(observation.status)
        ));
    }
    let mut files = Vec::<DiffFile>::new();
    let mut current = None;
    let mut patch_lines = 0_usize;
    for line in observation.stdout.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(file) = current.take() {
                files.push(file);
            }
            let path = rest.split_once(" b/").map(|(_, path)| path).unwrap_or(rest);
            current = Some(DiffFile {
                path: safe(path.trim_matches('"')),
                ..DiffFile::default()
            });
            patch_lines += 1;
        } else if let Some(file) = current.as_mut() {
            patch_lines += 1;
            if let Some(path) = line.strip_prefix("+++ b/") {
                file.path = safe(path.trim_matches('"'));
            } else if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
                file.binary = true;
            } else if line.starts_with('+') && !line.starts_with("+++") {
                file.added += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                file.deleted += 1;
            }
        } else if !line.trim().is_empty() {
            return None;
        }
    }
    if let Some(file) = current {
        files.push(file);
    }
    if files.is_empty() {
        return None;
    }
    let added = files.iter().map(|file| file.added).sum::<usize>();
    let deleted = files.iter().map(|file| file.deleted).sum::<usize>();
    let mut result = format!(
        "Git diff · {} {} · +{added} -{deleted} · {}",
        files.len(),
        if files.len() == 1 { "file" } else { "files" },
        terminal_status(observation.status)
    );
    for file in files.iter().take(MAX_LISTED_PATHS) {
        let binary = if file.binary { " · binary" } else { "" };
        result.push_str(&format!(
            "\n{} · +{} -{}{}",
            file.path, file.added, file.deleted, binary
        ));
    }
    if files.len() > MAX_LISTED_PATHS {
        result.push_str(&format!(
            "\n+{} files omitted from semantic summary",
            files.len() - MAX_LISTED_PATHS
        ));
    }
    result.push_str(&format!(
        "\n{patch_lines} captured patch lines omitted from collapsed view"
    ));
    Some(result)
}

fn summarize_cargo_test(observation: &CommandObservation) -> Option<String> {
    static RESULT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"test result: (?:ok|FAILED)\. ([0-9]+) passed; ([0-9]+) failed; ([0-9]+) ignored; ([0-9]+) measured; ([0-9]+) filtered out",
        )
        .expect("fixed cargo test summary regex")
    });
    let combined = format!("{}\n{}", observation.stdout, observation.stderr);
    let mut suites = 0_usize;
    let mut totals = [0_usize; 5];
    for captures in RESULT.captures_iter(&combined) {
        suites += 1;
        for (index, total) in totals.iter_mut().enumerate() {
            *total += captures[index + 1].parse::<usize>().ok()?;
        }
    }
    if suites == 0 {
        return None;
    }
    let mut failures = Vec::new();
    for line in combined.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed
            .strip_prefix("test ")
            .and_then(|value| value.strip_suffix(" ... FAILED"))
        {
            if !failures.iter().any(|existing| existing == name) {
                failures.push(safe(name));
            }
        }
    }
    let mut result = format!(
        "Cargo test · {} · {} suites\n{} passed · {} failed · {} ignored · {} measured · {} filtered out",
        terminal_status(observation.status),
        suites,
        totals[0],
        totals[1],
        totals[2],
        totals[3],
        totals[4]
    );
    if !failures.is_empty() {
        result.push_str("\nFailing tests:");
        for failure in failures.iter().take(MAX_LISTED_DIAGNOSTICS) {
            result.push_str(&format!("\n  {failure}"));
        }
        if failures.len() > MAX_LISTED_DIAGNOSTICS {
            result.push_str(&format!(
                "\n  +{} failures omitted from semantic summary",
                failures.len() - MAX_LISTED_DIAGNOSTICS
            ));
        }
    }
    let detail_lines = cargo_failure_detail_lines(&combined);
    if !detail_lines.is_empty() {
        result.push_str("\nBounded failure detail:");
        for line in detail_lines {
            result.push_str(&format!("\n  {}", safe(&line)));
        }
    }
    Some(result)
}

fn cargo_failure_detail_lines(output: &str) -> Vec<String> {
    let mut detail = Vec::new();
    let mut remaining = 0_usize;
    for line in output.lines() {
        if line.starts_with("---- ") && line.ends_with(" stdout ----") {
            remaining = 6;
        }
        if remaining > 0 {
            detail.push(line.to_string());
            remaining -= 1;
            if detail.len() == 24 {
                break;
            }
        }
    }
    detail
}

fn summarize_cargo_clippy(observation: &CommandObservation) -> Option<String> {
    let combined = format!("{}\n{}", observation.stdout, observation.stderr);
    let mut diagnostics = Vec::<(&'static str, String, Option<String>)>::new();
    for line in combined.lines() {
        let trimmed = line.trim();
        let (severity, rest) = if let Some(rest) = trimmed.strip_prefix("error") {
            ("error", rest)
        } else if let Some(rest) = trimmed.strip_prefix("warning") {
            ("warning", rest)
        } else {
            continue;
        };
        let (code, message) = if let Some(rest) = rest.strip_prefix('[') {
            rest.split_once("]:")
                .map_or((None, rest), |(code, message)| {
                    (Some(safe(code)), message.trim())
                })
        } else {
            (None, rest.strip_prefix(':').unwrap_or(rest).trim())
        };
        if !message.is_empty() {
            diagnostics.push((severity, safe(message), code));
        }
    }
    if diagnostics.is_empty() && !status_success(observation.status) {
        return None;
    }
    let errors = diagnostics
        .iter()
        .filter(|(severity, _, _)| *severity == "error")
        .count();
    let warnings = diagnostics.len().saturating_sub(errors);
    let mut result = format!(
        "Cargo Clippy · {}\n{errors} {} · {warnings} {}",
        terminal_status(observation.status),
        if errors == 1 { "error" } else { "errors" },
        if warnings == 1 { "warning" } else { "warnings" }
    );
    for (severity, message, code) in diagnostics.iter().take(MAX_LISTED_DIAGNOSTICS) {
        let code = code
            .as_deref()
            .map_or_else(String::new, |code| format!(" [{code}]"));
        result.push_str(&format!("\n{severity}{code}: {message}"));
    }
    if diagnostics.len() > MAX_LISTED_DIAGNOSTICS {
        result.push_str(&format!(
            "\n+{} diagnostics omitted from semantic summary",
            diagnostics.len() - MAX_LISTED_DIAGNOSTICS
        ));
    }
    Some(result)
}

fn terminal_status(status: crate::jobs::JobStatus) -> String {
    match status {
        crate::jobs::JobStatus::Completed { exit_code: 0 } => "completed (0)".into(),
        crate::jobs::JobStatus::Completed { exit_code }
        | crate::jobs::JobStatus::Failed {
            exit_code: Some(exit_code),
        } => format!("failed ({exit_code})"),
        crate::jobs::JobStatus::Failed { exit_code: None } => "failed".into(),
        crate::jobs::JobStatus::Cancelled => "cancelled".into(),
        crate::jobs::JobStatus::TimedOut => "timed out".into(),
        crate::jobs::JobStatus::Running => "running".into(),
        crate::jobs::JobStatus::Cancelling => "cancelling".into(),
    }
}

fn status_success(status: crate::jobs::JobStatus) -> bool {
    matches!(status, crate::jobs::JobStatus::Completed { exit_code: 0 })
}

fn safe(value: &str) -> String {
    crate::terminal_text::sanitize_terminal_text(value)
}

fn bound_summary(mut value: String) -> String {
    if value.len() <= MAX_SUMMARY_BYTES {
        return value;
    }
    let marker = "\n[semantic summary truncated]";
    let mut end = MAX_SUMMARY_BYTES
        .saturating_sub(marker.len())
        .min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value.push_str(marker);
    value
}

pub async fn present_full_rerun(
    rerun: FullOutputRerun,
    timeout: std::time::Duration,
    roots: Vec<std::path::PathBuf>,
    sandbox_mode: crate::sandbox::SandboxMode,
    owner: crate::jobs::JobOwner,
) -> Result<crate::jobs::JobStatus, String> {
    use std::io::Write as _;

    let manager = crate::jobs::JobManager::process();
    let mut events = manager.subscribe();
    let id = manager
        .start_sandboxed_owned(
            rerun.command.clone(),
            rerun.cwd.clone(),
            false,
            timeout,
            roots,
            rerun.network_requested,
            sandbox_mode,
            owner,
        )
        .await?;
    let heading = format!(
        "ca full command output\ncommand: {}\nworking directory: {}\n{}\n\n",
        safe(&rerun.command),
        safe(&rerun.cwd),
        if rerun.network_requested {
            "network: allowed for this confirmed rerun"
        } else {
            "network: disabled"
        }
    );
    print!("{heading}");
    let _ = std::io::stdout().flush();
    let status = loop {
        match events.recv().await {
            Ok(crate::jobs::JobEvent::Output {
                id: event_id, text, ..
            }) if event_id == id => {
                print!("{}", safe(&text));
                let _ = std::io::stdout().flush();
            }
            Ok(crate::jobs::JobEvent::Status {
                id: event_id,
                status,
            }) if event_id == id => break status,
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                print!(
                    "\n[{} output events omitted because the display fell behind]\n",
                    count
                );
                let _ = std::io::stdout().flush();
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                return Err("command output stream closed unexpectedly".into());
            }
        }
    };
    println!(
        "\n\nStatus: {}\n\nPress Enter to return to ca…",
        status.label()
    );
    let _ = std::io::stdout().flush();
    tokio::task::spawn_blocking(|| loop {
        if matches!(
            crossterm::event::read(),
            Ok(crossterm::event::Event::Key(crossterm::event::KeyEvent {
                code: crossterm::event::KeyCode::Enter,
                ..
            }))
        ) {
            break;
        }
    })
    .await
    .map_err(|error| format!("could not wait for terminal input: {error}"))?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_output_classification_is_closed_and_rejects_shell_ambiguity() {
        assert_eq!(
            classify_command("git status --short"),
            Some(CommandKind::GitStatus)
        );
        assert_eq!(
            classify_command("git diff -- src/lib.rs"),
            Some(CommandKind::GitDiff)
        );
        assert_eq!(
            classify_command("cargo test --all-targets -- --nocapture"),
            Some(CommandKind::CargoTest)
        );
        assert_eq!(
            classify_command("cargo clippy --all-targets"),
            Some(CommandKind::CargoClippy)
        );
        for command in [
            "git status | cat",
            "git diff > patch",
            "cargo test && echo done",
            "cargo test $(touch nope)",
            "env cargo test",
            "cargo +nightly test",
            "git -C elsewhere status",
            "cargo build",
        ] {
            assert_eq!(classify_command(command), None, "{command}");
        }
    }

    #[test]
    fn command_output_summaries_are_typed_bounded_and_truthful() {
        let status = observation(
            "git status --short",
            " M src/main.rs\nA  src/new.rs\n?? notes.txt\n",
            "",
            crate::jobs::JobStatus::Completed { exit_code: 0 },
        );
        let summary = summarize(&status).unwrap();
        assert!(summary.collapsed.contains("Git status · 3 paths"));
        assert!(summary.collapsed.contains("modified 1"));
        assert!(summary.collapsed.contains("added 1"));
        assert!(summary.collapsed.contains("untracked 1"));

        let diff = observation(
            "git diff",
            "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1,2 @@\n-old\n+new\n+more\n",
            "",
            crate::jobs::JobStatus::Completed { exit_code: 0 },
        );
        let summary = summarize(&diff).unwrap();
        assert!(summary.collapsed.contains("src/lib.rs · +2 -1"));
        assert!(summary.collapsed.contains("patch lines omitted"));

        let tests = observation(
            "cargo test",
            "test alpha ... ok\ntest beta ... FAILED\n\ntest result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n",
            "",
            crate::jobs::JobStatus::Failed { exit_code: Some(101) },
        );
        let summary = summarize(&tests).unwrap();
        assert!(summary.collapsed.contains("Cargo test · failed (101)"));
        assert!(summary.collapsed.contains("1 passed · 1 failed"));
        assert!(summary.collapsed.contains("beta"));

        let clippy = observation(
            "cargo clippy",
            "",
            "warning: unused variable: `x`\n  --> src/lib.rs:2:9\nerror[E0308]: mismatched types\n  --> src/lib.rs:3:5\n",
            crate::jobs::JobStatus::Failed { exit_code: Some(101) },
        );
        let summary = summarize(&clippy).unwrap();
        assert!(summary.collapsed.contains("Cargo Clippy · failed (101)"));
        assert!(summary.collapsed.contains("1 error · 1 warning"));
        assert!(summary.collapsed.contains("E0308"));
    }

    #[test]
    fn incomplete_capture_is_labeled_and_malformed_supported_output_falls_back() {
        let mut status = observation(
            "git status --short",
            " M src/main.rs\n",
            "",
            crate::jobs::JobStatus::Completed { exit_code: 0 },
        );
        status.stdout_omitted_bytes = 20;
        let summary = summarize(&status).unwrap();
        assert!(summary.collapsed.contains("source capture incomplete"));

        let malformed = observation(
            "git diff",
            "this is not a patch",
            "",
            crate::jobs::JobStatus::Completed { exit_code: 0 },
        );
        assert!(summarize(&malformed).is_none());
    }

    #[test]
    fn model_result_is_condensed_only_by_explicit_semantic_policy() {
        let observation = observation(
            "cargo test",
            "test alpha ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n",
            "",
            crate::jobs::JobStatus::Completed { exit_code: 0 },
        );
        let result = crate::agent::registry::ToolExecutionResult::Command {
            model_text: "bounded raw sentinel".into(),
            observation,
            success: true,
        };

        assert_eq!(
            model_text_for_policy(&result, crate::config::CommandOutputPolicy::Bounded),
            "bounded raw sentinel"
        );
        let semantic = model_text_for_policy(&result, crate::config::CommandOutputPolicy::Semantic);
        assert!(semantic.contains("Cargo test · completed (0)"));
        assert!(semantic.contains("1 passed · 0 failed"));
        assert!(!semantic.contains("bounded raw sentinel"));
        assert!(semantic.contains("process-local bounded raw capture"));
    }

    #[test]
    fn command_output_policy_defaults_and_comment_preserving_save() {
        let parsed: crate::config::CaConfig =
            toml::from_str("[agent]\n# keep me\nmax_turns = 7\n").unwrap();
        assert_eq!(
            parsed.agent.command_output,
            crate::config::CommandOutputPolicy::Bounded
        );
        assert!(toml::from_str::<crate::config::CaConfig>(
            "[agent]\ncommand_output = \"mystery\"\n"
        )
        .is_err());

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, "[agent]\n# keep me\nmax_turns = 7\n").unwrap();
        let store = crate::config::ConfigStore::new(&path);
        store
            .update_command_output(crate::config::CommandOutputPolicy::Semantic)
            .unwrap();
        let raw = std::fs::read_to_string(path).unwrap();
        assert!(raw.contains("# keep me"));
        assert!(raw.contains("command_output = \"semantic\""));
        assert_eq!(
            store.load().unwrap().agent.command_output,
            crate::config::CommandOutputPolicy::Semantic
        );
    }

    fn observation(
        command: &str,
        stdout: &str,
        stderr: &str,
        status: crate::jobs::JobStatus,
    ) -> CommandObservation {
        CommandObservation {
            command: command.into(),
            cwd: "/workspace".into(),
            network_requested: false,
            status,
            stdout: stdout.into(),
            stderr: stderr.into(),
            stdout_omitted_bytes: 0,
            stderr_omitted_bytes: 0,
            bounded_model_text: format!("{stdout}{stderr}"),
        }
    }
}
