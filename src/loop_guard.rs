use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::sync::LazyLock;

const EDIT_WINDOW: usize = 12;
const FAILURE_WINDOW: usize = 24;
const OUTPUT_LIMIT_CHARS: usize = 2_000;
const EVIDENCE_LIMIT_CHARS: usize = 160;
const EDIT_SIMILARITY_THRESHOLD: f64 = 0.8;

static TEMP_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:/private)?/(?:tmp|var/(?:tmp|folders))/[^\s'"`]+"#)
        .expect("valid temporary-path regex")
});
static ISO_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b\d{4}-\d{2}-\d{2}[t ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:z|[+-]\d{2}:?\d{2})?\b")
        .expect("valid timestamp regex")
});
static CLOCK_TIMESTAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{1,2}:\d{2}:\d{2}(?:\.\d+)?\b").expect("valid clock regex"));
static SEED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:--?seed(?:\s+|=)|\bseed\s*[:=]\s*)\d+\b").expect("valid seed regex")
});
static DURATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b\d+(?:\.\d+)?\s*(?:ns|us|µs|ms|msec|s|sec|secs|seconds?|minutes?|mins?)\b")
        .expect("valid duration regex")
});
static LARGE_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{6,}\b").expect("valid large-number regex"));
static TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[[:alnum:]_<>]+|[^\s[:alnum:]_]").expect("valid token regex"));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopIncidentKind {
    RepeatedEdit,
    RepeatedFailedCommand,
    RepeatedFailedMutation,
    NoProgressStreak,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopIncident {
    pub kind: LoopIncidentKind,
    pub evidence: String,
    pub occurrences: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopGuardDecision {
    Replan,
    Continue,
    Cancel,
}

impl LoopGuardDecision {
    pub fn previous(self) -> Self {
        match self {
            Self::Replan => Self::Cancel,
            Self::Continue => Self::Replan,
            Self::Cancel => Self::Continue,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Replan => Self::Continue,
            Self::Continue => Self::Cancel,
            Self::Cancel => Self::Replan,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopGuardInteraction {
    pub incident: LoopIncident,
    pub selected: LoopGuardDecision,
    pub cancel_confirmation: bool,
    pub cancel_confirm_selected: bool,
}

impl LoopGuardInteraction {
    pub fn new(incident: LoopIncident) -> Self {
        Self {
            incident,
            selected: LoopGuardDecision::Replan,
            cancel_confirmation: false,
            cancel_confirm_selected: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopObservation {
    Edit {
        target: String,
        body: String,
        changed: bool,
    },
    Command {
        command: String,
        output: String,
        success: bool,
        verification: bool,
    },
    MutationFailure {
        mutation: String,
        output: String,
    },
    Other,
}

impl LoopObservation {
    pub fn from_resolved(
        call: &crate::app::ToolCall,
        result: &crate::app::ResolvedToolResult,
    ) -> Self {
        let arguments = serde_json::from_str::<serde_json::Value>(&call.arguments)
            .unwrap_or_else(|_| serde_json::json!({}));
        if matches!(
            call.name.as_str(),
            "write_file"
                | "edit_file"
                | "hashline_edit"
                | "apply_patch"
                | "delete_file"
                | "create_directory"
        ) && result.status == crate::app::ResolvedToolStatus::Error
        {
            return Self::MutationFailure {
                mutation: format!("{}:{}", call.name, argument_string(&arguments, "path")),
                output: result.output.clone(),
            };
        }
        match call.name.as_str() {
            "write_file" if result.status == crate::app::ResolvedToolStatus::Done => {
                Self::edit_from_arguments(&arguments, "content", mutation_changed(result))
            }
            "edit_file" => {
                if result.status != crate::app::ResolvedToolStatus::Done {
                    return Self::Other;
                }
                let old = arguments
                    .get("old_string")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let new = arguments
                    .get("new_string")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                Self::Edit {
                    target: argument_string(&arguments, "path"),
                    body: format!("old {old}\nnew {new}"),
                    changed: mutation_changed(result),
                }
            }
            "hashline_edit" if result.status == crate::app::ResolvedToolStatus::Done => {
                Self::edit_from_arguments(&arguments, "replacement", mutation_changed(result))
            }
            "apply_patch" if result.status == crate::app::ResolvedToolStatus::Done => {
                Self::edit_from_arguments(&arguments, "patch", mutation_changed(result))
            }
            _ => result
                .command_observation
                .as_ref()
                .map_or(Self::Other, |observation| Self::Command {
                    command: observation.command.clone(),
                    output: command_output(observation),
                    success: result.status == crate::app::ResolvedToolStatus::Done,
                    verification: is_verification_command(&observation.command),
                }),
        }
    }

    pub fn from_execution(
        tool_name: &str,
        tool_arguments: &str,
        result: &crate::agent::registry::ToolExecutionResult,
    ) -> Self {
        if matches!(
            result,
            crate::agent::registry::ToolExecutionResult::Denied(_)
                | crate::agent::registry::ToolExecutionResult::Cancelled(_)
        ) {
            return Self::Other;
        }
        let resolved = crate::app::ResolvedToolResult {
            tool_id: String::new(),
            output: result.model_text().to_string(),
            status: if result.is_success() {
                crate::app::ResolvedToolStatus::Done
            } else {
                crate::app::ResolvedToolStatus::Error
            },
            image: None,
            memory_proposal: None,
            mutation: result.mutation_batch().cloned(),
            command_observation: result.command_observation().cloned(),
        };
        let call = crate::app::ToolCall {
            id: String::new(),
            name: tool_name.to_string(),
            arguments: tool_arguments.to_string(),
            status: crate::app::ToolStatus::Running,
            output: None,
        };
        Self::from_resolved(&call, &resolved)
    }

    fn edit_from_arguments(arguments: &serde_json::Value, body_key: &str, changed: bool) -> Self {
        Self::Edit {
            target: argument_string(arguments, "path"),
            body: argument_string(arguments, body_key),
            changed,
        }
    }
}

fn mutation_changed(result: &crate::app::ResolvedToolResult) -> bool {
    result.mutation.as_ref().is_some_and(|batch| {
        batch
            .receipts()
            .iter()
            .any(|receipt| receipt.before != receipt.after)
    })
}

fn argument_string(arguments: &serde_json::Value, key: &str) -> String {
    arguments
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn command_output(observation: &crate::command_output::CommandObservation) -> String {
    let mut output = String::with_capacity(
        observation
            .stdout
            .len()
            .saturating_add(observation.stderr.len())
            .min(OUTPUT_LIMIT_CHARS),
    );
    output.push_str(&observation.stdout);
    if !observation.stdout.is_empty() && !observation.stderr.is_empty() {
        output.push('\n');
    }
    output.push_str(&observation.stderr);
    if observation.stdout_omitted_bytes > 0 || observation.stderr_omitted_bytes > 0 {
        output.push_str("\n<omitted-output>");
    }
    bound_chars_head_tail(&output, OUTPUT_LIMIT_CHARS)
}

#[derive(Debug, Clone)]
struct EditRecord {
    target: String,
    shingles: HashSet<String>,
}

#[derive(Debug, Clone)]
struct FailureRecord {
    command: String,
    output: String,
}

#[derive(Debug, Default)]
pub struct LoopGuard {
    edits: VecDeque<EditRecord>,
    failures: VecDeque<FailureRecord>,
    last_verification: Option<(String, String)>,
    no_progress_streak: u8,
    suppressed: bool,
    incident_latched: bool,
}

impl LoopGuard {
    pub fn observe_batch<I>(&mut self, observations: I) -> Option<LoopIncident>
    where
        I: IntoIterator<Item = LoopObservation>,
    {
        if self.suppressed || self.incident_latched {
            return None;
        }
        for observation in observations {
            if let Some(incident) = self.observe(observation) {
                self.incident_latched = true;
                return Some(incident);
            }
        }
        None
    }

    pub fn reset_for_turn(&mut self) {
        *self = Self::default();
    }

    pub fn replan(&mut self) {
        self.clear_observations();
        self.suppressed = false;
    }

    pub fn suppress_for_turn(&mut self) {
        self.clear_observations();
        self.suppressed = true;
    }

    pub fn is_suppressed(&self) -> bool {
        self.suppressed
    }

    fn clear_observations(&mut self) {
        self.edits.clear();
        self.failures.clear();
        self.last_verification = None;
        self.no_progress_streak = 0;
        self.incident_latched = false;
    }

    fn observe(&mut self, observation: LoopObservation) -> Option<LoopIncident> {
        match observation {
            LoopObservation::Edit {
                target,
                body,
                changed,
            } => self.observe_edit(target, body, changed),
            LoopObservation::Command {
                command,
                output,
                success,
                verification,
            } => self.observe_command(command, output, success, verification),
            LoopObservation::MutationFailure { mutation, output } => {
                self.observe_mutation_failure(mutation, output)
            }
            LoopObservation::Other => {
                self.no_progress_streak = 0;
                None
            }
        }
    }

    fn observe_edit(
        &mut self,
        target: String,
        body: String,
        changed: bool,
    ) -> Option<LoopIncident> {
        let normalized_target = normalize_text(&target);
        let shingles = shingles(&normalize_edit_body(&body));
        let similar = self
            .edits
            .iter()
            .filter(|record| {
                record.target == normalized_target
                    && jaccard(&record.shingles, &shingles) > EDIT_SIMILARITY_THRESHOLD
            })
            .count();
        self.edits.push_back(EditRecord {
            target: normalized_target,
            shingles,
        });
        while self.edits.len() > EDIT_WINDOW {
            self.edits.pop_front();
        }
        if similar >= 2 {
            return Some(LoopIncident {
                kind: LoopIncidentKind::RepeatedEdit,
                evidence: format!(
                    "Near-identical edit to {} repeated 3 times",
                    display_evidence(&target)
                ),
                occurrences: 3,
            });
        }
        if changed {
            self.no_progress_streak = 0;
            return None;
        }
        self.no_progress_streak = self.no_progress_streak.saturating_add(1);
        self.streak_incident()
    }

    fn observe_command(
        &mut self,
        command: String,
        output: String,
        success: bool,
        verification: bool,
    ) -> Option<LoopIncident> {
        let normalized_command = normalize_text(&command);
        let normalized_output = normalize_text(&bound_chars_head_tail(&output, OUTPUT_LIMIT_CHARS));
        let previous_verification = self.last_verification.clone();

        if !success {
            let repeats = self.record_failure(&normalized_command, &normalized_output);
            if repeats >= 2 {
                return Some(LoopIncident {
                    kind: LoopIncidentKind::RepeatedFailedCommand,
                    evidence: format!(
                        "Failing command `{}` returned unchanged output 3 times",
                        display_evidence(&command)
                    ),
                    occurrences: 3,
                });
            }
        }

        if !verification {
            self.no_progress_streak = 0;
            return None;
        }
        self.last_verification = Some((normalized_command.clone(), normalized_output.clone()));
        let observable_progress = success
            || previous_verification.is_some_and(|(previous_command, previous_output)| {
                previous_command == normalized_command && previous_output != normalized_output
            });
        if observable_progress {
            self.no_progress_streak = 0;
            return None;
        }
        self.no_progress_streak = self.no_progress_streak.saturating_add(1);
        self.streak_incident()
    }

    fn observe_mutation_failure(
        &mut self,
        mutation: String,
        output: String,
    ) -> Option<LoopIncident> {
        let normalized_mutation = normalize_text(&mutation);
        let normalized_output = normalize_text(&bound_chars_head_tail(&output, OUTPUT_LIMIT_CHARS));
        let repeats = self.record_failure(&normalized_mutation, &normalized_output);
        self.no_progress_streak = self.no_progress_streak.saturating_add(1);
        if repeats >= 2 {
            return Some(LoopIncident {
                kind: LoopIncidentKind::RepeatedFailedMutation,
                evidence: format!(
                    "Mutation `{}` failed with unchanged output 3 times",
                    display_evidence(&mutation)
                ),
                occurrences: 3,
            });
        }
        self.streak_incident()
    }

    fn record_failure(&mut self, command: &str, output: &str) -> usize {
        let repeats = self
            .failures
            .iter()
            .filter(|record| record.command == command && record.output == output)
            .count();
        self.failures.push_back(FailureRecord {
            command: command.to_string(),
            output: output.to_string(),
        });
        while self.failures.len() > FAILURE_WINDOW {
            self.failures.pop_front();
        }
        repeats
    }

    fn streak_incident(&self) -> Option<LoopIncident> {
        (self.no_progress_streak >= 4).then(|| LoopIncident {
            kind: LoopIncidentKind::NoProgressStreak,
            evidence: "Four consecutive edit or verification actions showed no progress".into(),
            occurrences: self.no_progress_streak,
        })
    }
}

fn normalize_text(value: &str) -> String {
    let value = bound_chars_head_tail(value, OUTPUT_LIMIT_CHARS);
    let value = TEMP_PATH.replace_all(&value, "<temp-path>");
    let value = ISO_TIMESTAMP.replace_all(&value, "<timestamp>");
    let value = CLOCK_TIMESTAMP.replace_all(&value, "<timestamp>");
    let value = SEED.replace_all(&value, "seed=<seed>");
    let value = DURATION.replace_all(&value, "<duration>");
    let value = LARGE_NUMBER.replace_all(&value, "<number>");
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn normalize_edit_body(value: &str) -> String {
    let meaningful = value
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            !line.starts_with("@@") && !line.starts_with("--- ") && !line.starts_with("+++ ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    normalize_text(&meaningful)
}

fn shingles(value: &str) -> HashSet<String> {
    let tokens = TOKEN
        .find_iter(value)
        .map(|token| token.as_str())
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return HashSet::from([String::new()]);
    }
    if tokens.len() < 3 {
        return HashSet::from([tokens.join("\u{1f}")]);
    }
    tokens
        .windows(3)
        .map(|window| window.join("\u{1f}"))
        .collect()
}

fn jaccard(left: &HashSet<String>, right: &HashSet<String>) -> f64 {
    let union = left.union(right).count();
    if union == 0 {
        return 1.0;
    }
    left.intersection(right).count() as f64 / union as f64
}

fn display_evidence(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    bound_chars_head_tail(sanitized.trim(), EVIDENCE_LIMIT_CHARS)
}

fn bound_chars_head_tail(value: &str, limit: usize) -> String {
    let count = value.chars().count();
    if count <= limit {
        return value.to_string();
    }
    let head = limit / 2;
    let tail = limit.saturating_sub(head + 1);
    let start = value.chars().take(head).collect::<String>();
    let end = value
        .chars()
        .rev()
        .take(tail)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{start}…{end}")
}

fn is_verification_command(command: &str) -> bool {
    let normalized = command.trim().to_ascii_lowercase();
    [
        "cargo test",
        "cargo check",
        "cargo clippy",
        "cargo build",
        "cargo fmt --check",
        "pytest",
        "python -m pytest",
        "go test",
        "npm test",
        "npm run test",
        "pnpm test",
        "pnpm run test",
        "yarn test",
        "bun test",
        "make test",
    ]
    .iter()
    .any(|prefix| {
        normalized == *prefix
            || normalized
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with(char::is_whitespace))
    })
}

/// Experimental hints are observed separately from the established loop guard.
/// A task can emit at most two suggestions and each pattern at most once.
#[derive(Debug, Clone, Default)]
pub struct ExecutionHints {
    dependency_probes: u8,
    foreground_waits: u8,
    build_only_checks: u8,
    emitted: HashSet<&'static str>,
}
impl ExecutionHints {
    pub fn observe(&mut self, command: &str) -> Option<(&'static str, &'static str)> {
        if self.emitted.len() >= 2 {
            return None;
        }
        let command = command.trim();
        let (id, count, hint) = if command.starts_with("which ")
            || command.starts_with("command -v ")
            || command.ends_with(" --version")
        {
            self.dependency_probes = self.dependency_probes.saturating_add(1);
            ("dependency_probes",self.dependency_probes,"Combine dependency checks and use the results already gathered before probing again.")
        } else if command.starts_with("sleep ") {
            self.foreground_waits = self.foreground_waits.saturating_add(1);
            (
                "foreground_waits",
                self.foreground_waits,
                "Use a bounded job-status check while long-running work continues.",
            )
        } else if command.starts_with("cargo check")
            || command.starts_with("cargo build")
            || command == "git diff --check"
        {
            self.build_only_checks = self.build_only_checks.saturating_add(1);
            ("build_only_checks",self.build_only_checks,"Compilation and formatting checks do not verify the requested behavior. Run its focused acceptance check.")
        } else {
            return None;
        };
        (count >= 3 && self.emitted.insert(id)).then_some((id, hint))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(path: &str, body: &str) -> LoopObservation {
        LoopObservation::Edit {
            target: path.into(),
            body: body.into(),
            changed: false,
        }
    }

    fn changed_edit(path: &str, body: &str) -> LoopObservation {
        LoopObservation::Edit {
            target: path.into(),
            body: body.into(),
            changed: true,
        }
    }

    fn command(command: &str, output: &str, success: bool) -> LoopObservation {
        LoopObservation::Command {
            command: command.into(),
            output: output.into(),
            success,
            verification: is_verification_command(command),
        }
    }

    #[test]
    fn normalization_removes_expected_nondeterminism_and_bounds_output() {
        let left = normalize_text(
            "failed /tmp/ca-aQ3/file at 2026-08-16T12:34:56.100Z seed=12345 in 651ms id 1234567",
        );
        let right = normalize_text(
            "failed /private/tmp/ca-zX9/file at 2026-08-17T01:02:03Z seed: 99999 in 3.4s id 7654321",
        );
        assert_eq!(left, right);
        assert!(normalize_text(&"x".repeat(8_000)).chars().count() <= OUTPUT_LIMIT_CHARS);
    }

    #[test]
    fn third_similar_edit_to_same_target_triggers() {
        let mut guard = LoopGuard::default();
        let base = "@@ -1,3 +1,3 @@ fn answer() { let first = alpha; let second = beta; let third = gamma; return old_value; }";
        assert_eq!(guard.observe_batch([edit("src/lib.rs", base)]), None);
        assert_eq!(
            guard.observe_batch([edit(
                "src/lib.rs",
                "@@ -20,3 +20,3 @@ fn answer() { let first = alpha; let second = beta; let third = gamma; return old_value; }"
            )]),
            None
        );
        let incident = guard
            .observe_batch([edit("src/lib.rs", base)])
            .expect("third similar edit should pause");
        assert_eq!(incident.kind, LoopIncidentKind::RepeatedEdit);
        assert_eq!(incident.occurrences, 3);
    }

    #[test]
    fn edit_window_target_and_similarity_prevent_false_positive() {
        let mut guard = LoopGuard::default();
        guard.observe_batch([edit("src/a.rs", "alpha beta gamma delta")]);
        guard.observe_batch([edit("src/b.rs", "alpha beta gamma delta")]);
        assert_eq!(
            guard.observe_batch([edit("src/a.rs", "completely different replacement")]),
            None
        );
        guard.reset_for_turn();
        for index in 0..12 {
            guard.observe_batch([edit("src/a.rs", &format!("unique body {index} value"))]);
        }
        assert_eq!(
            guard.observe_batch([edit("src/a.rs", "alpha beta gamma delta")]),
            None
        );
    }

    #[test]
    fn third_unchanged_failure_triggers_despite_noise() {
        let mut guard = LoopGuard::default();
        assert_eq!(
            guard.observe_batch([command(
                "cargo test --seed 10",
                "failed at /tmp/a in 50ms 123456",
                false,
            )]),
            None
        );
        assert_eq!(
            guard.observe_batch([command(
                "cargo test --seed 20",
                "failed at /tmp/b in 60ms 999999",
                false,
            )]),
            None
        );
        let incident = guard
            .observe_batch([command(
                "cargo test --seed 30",
                "failed at /tmp/c in 70ms 777777",
                false,
            )])
            .expect("third unchanged failure should pause");
        assert_eq!(incident.kind, LoopIncidentKind::RepeatedFailedCommand);
    }

    #[test]
    fn third_identical_mutation_failure_triggers() {
        let mut guard = LoopGuard::default();
        let failure = LoopObservation::MutationFailure {
            mutation: "apply_patch:src/lib.rs".into(),
            output: "file unchanged; re-read and regenerate the patch".into(),
        };
        assert_eq!(guard.observe_batch([failure.clone()]), None);
        assert_eq!(guard.observe_batch([failure.clone()]), None);
        let incident = guard
            .observe_batch([failure])
            .expect("third identical mutation failure should pause");
        assert_eq!(incident.kind, LoopIncidentKind::RepeatedFailedMutation);
        assert_eq!(incident.occurrences, 3);
    }

    #[test]
    fn changed_failure_and_success_are_progress() {
        let mut guard = LoopGuard::default();
        guard.observe_batch([command("cargo test", "error one", false)]);
        guard.observe_batch([command("cargo test", "error two", false)]);
        assert_eq!(
            guard.observe_batch([command("cargo test", "ok", true)]),
            None
        );
        assert_eq!(
            guard.observe_batch([command("cargo test", "error one", false)]),
            None
        );
    }

    #[test]
    fn fourth_no_progress_edit_or_verification_action_triggers() {
        let mut guard = LoopGuard::default();
        guard.observe_batch([edit("src/a.rs", "one")]);
        guard.observe_batch([command("cargo test", "first failure", false)]);
        guard.observe_batch([edit("src/a.rs", "two")]);
        let incident = guard
            .observe_batch([command("cargo test", "first failure", false)])
            .expect("fourth no-progress action should pause");
        assert_eq!(incident.kind, LoopIncidentKind::NoProgressStreak);
        assert_eq!(incident.occurrences, 4);
    }

    #[test]
    fn four_distinct_successful_byte_changing_edits_are_progress() {
        let mut guard = LoopGuard::default();
        for (path, body) in [
            ("src/a.rs", "replace alpha with beta"),
            ("src/b.rs", "add a bounded parser"),
            ("src/c.rs", "wire the progress event"),
            ("src/d.rs", "cover the recovery path"),
        ] {
            assert_eq!(guard.observe_batch([changed_edit(path, body)]), None);
        }
    }

    #[test]
    fn successful_mutation_receipt_marks_edit_as_changed() {
        let call = crate::app::ToolCall {
            id: "call-1".into(),
            name: "edit_file".into(),
            arguments: serde_json::json!({
                "path": "src/lib.rs",
                "old_string": "old",
                "new_string": "new"
            })
            .to_string(),
            status: crate::app::ToolStatus::Running,
            output: None,
        };
        let result = crate::app::ResolvedToolResult {
            tool_id: call.id.clone(),
            output: "updated".into(),
            status: crate::app::ResolvedToolStatus::Done,
            image: None,
            memory_proposal: None,
            mutation: Some(
                crate::undo::FileMutationReceipt::new(
                    "src/lib.rs".into(),
                    "edit",
                    crate::undo::FileState::Utf8("old".into()),
                    crate::undo::FileState::Utf8("new".into()),
                )
                .into(),
            ),
            command_observation: None,
        };

        assert!(matches!(
            LoopObservation::from_resolved(&call, &result),
            LoopObservation::Edit { changed: true, .. }
        ));
    }

    #[test]
    fn unrelated_action_resets_no_progress_streak() {
        let mut guard = LoopGuard::default();
        guard.observe_batch([edit("src/a.rs", "one")]);
        guard.observe_batch([edit("src/a.rs", "two")]);
        guard.observe_batch([LoopObservation::Other]);
        guard.observe_batch([edit("src/a.rs", "three")]);
        assert_eq!(guard.observe_batch([edit("src/a.rs", "four")]), None);
    }

    #[test]
    fn replan_resets_while_continue_suppresses_only_current_turn() {
        let mut guard = LoopGuard::default();
        for body in ["one", "two", "three"] {
            guard.observe_batch([edit("src/a.rs", body)]);
        }
        let _ = guard.observe_batch([edit("src/a.rs", "four")]);
        guard.replan();
        assert!(!guard.is_suppressed());
        assert_eq!(guard.observe_batch([edit("src/a.rs", "fresh")]), None);
        guard.suppress_for_turn();
        assert!(guard.is_suppressed());
        assert_eq!(
            guard.observe_batch(vec![command("cargo test", "same", false); 3]),
            None
        );
        guard.reset_for_turn();
        assert!(!guard.is_suppressed());
    }

    #[test]
    fn decisions_wrap_deterministically() {
        assert_eq!(
            LoopGuardDecision::Replan.previous(),
            LoopGuardDecision::Cancel
        );
        assert_eq!(LoopGuardDecision::Cancel.next(), LoopGuardDecision::Replan);
    }
}

#[cfg(test)]
mod execution_hint_tests {
    use super::*;
    #[test]
    fn hrs253_hints_are_bounded_and_do_not_repeat() {
        let mut hints = ExecutionHints::default();
        assert!(hints.observe("which cargo").is_none());
        assert!(hints.observe("which rustc").is_none());
        assert_eq!(
            hints.observe("cargo --version").unwrap().0,
            "dependency_probes"
        );
        assert!(hints.observe("which cargo").is_none());
        for _ in 0..2 {
            assert!(hints.observe("sleep 10").is_none());
        }
        assert_eq!(hints.observe("sleep 10").unwrap().0, "foreground_waits");
        for _ in 0..10 {
            assert!(hints.observe("cargo check").is_none());
        }
    }
}
