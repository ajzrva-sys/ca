use crate::agent::client::{ChatBackend, ChatMessage, ToolDef, ToolFunction};
use crate::agent::stream::AssistantTurnAssembler;
use crate::changes::{GitChangeService, ReviewInput, ReviewTarget};
use crate::terminal_text::sanitize_terminal_text;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub const MAX_WALKTHROUGH_FILES: usize = 128;
pub const MAX_WALKTHROUGH_STEPS: usize = 32;
pub const MAX_WALKTHROUGH_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_SUMMARY_BYTES: usize = 4 * 1024;
const MAX_TITLE_BYTES: usize = 256;
const MAX_EXPLANATION_BYTES: usize = 8 * 1024;
const MAX_NOTE_BYTES: usize = 1024;
const MAX_NOTES: usize = 16;
const MAX_FILES_PER_STEP: usize = 16;
const MAX_REFUSAL_PATHS: usize = 128;

#[derive(Clone, PartialEq, Eq)]
pub struct ChangeSnapshot {
    pub target: ReviewTarget,
    pub scope: String,
    pub source_hash: String,
    pub included_files: Vec<String>,
    diff: String,
}

impl std::fmt::Debug for ChangeSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ChangeSnapshot")
            .field("target", &self.target)
            .field("scope", &self.scope)
            .field("source_hash", &self.source_hash)
            .field("included_files", &self.included_files)
            .field("diff", &"[OMITTED]")
            .finish()
    }
}

impl ChangeSnapshot {
    fn prompt(&self) -> String {
        format!(
            "<change_snapshot version=\"1\" sha256=\"{}\">\n<scope>{}</scope>\n<files>\n{}\n</files>\n<diff>\n{}\n</diff>\n</change_snapshot>",
            self.source_hash,
            self.scope,
            self.included_files.join("\n"),
            self.diff
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkthroughRefusal {
    pub reason: String,
    pub omitted_files: Vec<String>,
    pub untracked_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotCapture {
    Complete(ChangeSnapshot),
    Refused(WalkthroughRefusal),
}

pub async fn capture_snapshot(
    service: &GitChangeService,
    target: ReviewTarget,
) -> Result<SnapshotCapture, String> {
    if matches!(target, ReviewTarget::Custom(_)) {
        return Err("Custom instructions are not a walkthrough target".into());
    }
    let input = service.review_input(target.clone()).await?;
    Ok(snapshot_from_review_input(target, input))
}

fn snapshot_from_review_input(target: ReviewTarget, input: ReviewInput) -> SnapshotCapture {
    let mut omitted_files = input.omitted_files;
    if input.included_files.len() > MAX_WALKTHROUGH_FILES {
        omitted_files.extend(
            input
                .included_files
                .iter()
                .skip(MAX_WALKTHROUGH_FILES)
                .cloned(),
        );
    }
    let omitted_files = bounded_paths(omitted_files);
    let untracked_files = bounded_paths(input.untracked_files);
    let reason = if !untracked_files.is_empty() {
        Some("Untracked file contents are not captured; stage them or remove them before generating a complete walkthrough")
    } else if !omitted_files.is_empty() {
        Some("The change snapshot is incomplete because tracked files exceeded the walkthrough bounds; narrow the target before retrying")
    } else if input.included_files.is_empty() || input.diff.trim().is_empty() {
        Some("No textual tracked changes are available for this target")
    } else {
        None
    };
    if let Some(reason) = reason {
        return SnapshotCapture::Refused(WalkthroughRefusal {
            reason: reason.into(),
            omitted_files,
            untracked_files,
        });
    }

    let scope = bounded_text(&sanitize_terminal_text(&input.scope), 512);
    let included_files = input
        .included_files
        .into_iter()
        .take(MAX_WALKTHROUGH_FILES)
        .map(|path| bounded_text(&sanitize_terminal_text(&path), 4096))
        .collect::<Vec<_>>();
    let diff = sanitize_terminal_text(&input.diff);
    let source_hash = source_hash(&target, &scope, &included_files, &diff);
    SnapshotCapture::Complete(ChangeSnapshot {
        target,
        scope,
        source_hash,
        included_files,
        diff,
    })
}

fn source_hash(target: &ReviewTarget, scope: &str, files: &[String], diff: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"ca.change-walkthrough.snapshot.v1\0");
    hasher.update(target_label(target).as_bytes());
    hasher.update(b"\0");
    hasher.update(scope.as_bytes());
    for file in files {
        hasher.update(b"\0file\0");
        hasher.update(file.as_bytes());
    }
    hasher.update(b"\0diff\0");
    hasher.update(diff.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn target_label(target: &ReviewTarget) -> String {
    match target {
        ReviewTarget::Working => "working".into(),
        ReviewTarget::Base(reference) => format!("base {reference}"),
        ReviewTarget::Commit(sha) => format!("commit {sha}"),
        ReviewTarget::Custom(_) => "custom".into(),
    }
}

fn bounded_paths(paths: Vec<String>) -> Vec<String> {
    let total = paths.len();
    let mut bounded = paths
        .into_iter()
        .take(MAX_REFUSAL_PATHS)
        .map(|path| bounded_text(&sanitize_terminal_text(&path), 4096))
        .collect::<Vec<_>>();
    if total > MAX_REFUSAL_PATHS {
        bounded.push(format!(
            "[{} additional paths omitted from display]",
            total - MAX_REFUSAL_PATHS
        ));
    }
    bounded
}

fn bounded_text(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WalkthroughStep {
    pub order: u8,
    pub title: String,
    #[serde(default)]
    pub files: Vec<String>,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChangeWalkthroughResult {
    pub summary: String,
    pub steps: Vec<WalkthroughStep>,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl ChangeWalkthroughResult {
    fn validate(mut self, snapshot: &ChangeSnapshot) -> Result<Self, String> {
        self.summary = checked_text(self.summary, MAX_SUMMARY_BYTES, "summary")?;
        if self.steps.is_empty() || self.steps.len() > MAX_WALKTHROUGH_STEPS {
            return Err(format!(
                "walkthrough must contain 1-{MAX_WALKTHROUGH_STEPS} steps"
            ));
        }
        if self.notes.len() > MAX_NOTES {
            return Err(format!("walkthrough may contain at most {MAX_NOTES} notes"));
        }
        let allowed = snapshot
            .included_files
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        for (index, step) in self.steps.iter_mut().enumerate() {
            let expected = u8::try_from(index + 1).expect("step count is bounded to 32");
            if step.order != expected {
                return Err("walkthrough step order must be consecutive starting at 1".into());
            }
            step.title = checked_text(std::mem::take(&mut step.title), MAX_TITLE_BYTES, "title")?;
            step.explanation = checked_text(
                std::mem::take(&mut step.explanation),
                MAX_EXPLANATION_BYTES,
                "explanation",
            )?;
            if step.files.len() > MAX_FILES_PER_STEP {
                return Err(format!(
                    "a walkthrough step may reference at most {MAX_FILES_PER_STEP} files"
                ));
            }
            for file in &mut step.files {
                let raw = file.trim();
                let safe = bounded_text(&sanitize_terminal_text(raw), 4096);
                if safe != raw {
                    return Err("walkthrough file references must be exact safe paths".into());
                }
                *file = safe;
                if !allowed.contains(file.as_str()) {
                    return Err(format!("walkthrough referenced an uncaptured file: {file}"));
                }
            }
        }
        for note in &mut self.notes {
            *note = checked_text(std::mem::take(note), MAX_NOTE_BYTES, "note")?;
        }
        let rendered = serde_json::to_vec(&self)
            .map_err(|_| "walkthrough result could not be bounded".to_string())?;
        if rendered.len() > MAX_WALKTHROUGH_OUTPUT_BYTES {
            return Err("walkthrough result exceeds the 64-KiB output limit".into());
        }
        Ok(self)
    }

    pub fn render(&self) -> String {
        let mut output = self.summary.clone();
        for step in &self.steps {
            output.push_str(&format!("\n\n{}. {}", step.order, step.title));
            if !step.files.is_empty() {
                output.push_str(&format!("\nFiles: {}", step.files.join(", ")));
            }
            output.push_str(&format!("\n{}", step.explanation));
        }
        if !self.notes.is_empty() {
            output.push_str("\n\nNotes");
            for note in &self.notes {
                output.push_str(&format!("\n- {note}"));
            }
        }
        output
    }
}

fn checked_text(value: String, max: usize, label: &str) -> Result<String, String> {
    let value = sanitize_terminal_text(value.trim());
    if value.is_empty() || value.len() > max {
        return Err(format!("{label} must be 1-{max} bytes"));
    }
    Ok(value)
}

pub fn report_tool() -> ToolDef {
    ToolDef {
        def_type: "function".into(),
        function: ToolFunction {
            name: "submit_change_walkthrough".into(),
            description:
                "Submit the complete ordered explanation for the exact supplied change snapshot"
                    .into(),
            parameters: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["summary", "steps", "notes"],
                "properties": {
                    "summary": { "type": "string" },
                    "steps": {
                        "type": "array", "minItems": 1, "maxItems": MAX_WALKTHROUGH_STEPS,
                        "items": {
                            "type": "object", "additionalProperties": false,
                            "required": ["order", "title", "files", "explanation"],
                            "properties": {
                                "order": { "type": "integer", "minimum": 1, "maximum": MAX_WALKTHROUGH_STEPS },
                                "title": { "type": "string" },
                                "files": { "type": "array", "maxItems": MAX_FILES_PER_STEP, "items": { "type": "string" } },
                                "explanation": { "type": "string" }
                            }
                        }
                    },
                    "notes": { "type": "array", "maxItems": MAX_NOTES, "items": { "type": "string" } }
                }
            }),
        },
    }
}

pub async fn run_walkthrough<B: ChatBackend + ?Sized>(
    backend: &B,
    route: (&str, &str),
    snapshot: &ChangeSnapshot,
    options: crate::agent::client::ChatRequestOptions,
    usage_recorder: crate::usage::UsageRecorder,
) -> Result<ChangeWalkthroughResult, String> {
    let (provider, model) = route;
    let messages = vec![
        ChatMessage {
            role: "system".into(),
            content: crate::image_content::ModelContent::text(
                "Explain the exact supplied Git snapshot as an ordered code walkthrough for its author. Describe intent, data flow, and important interactions without inventing omitted context or reporting review findings. Do not request files, use tools, propose edits, or mention chat history. Call submit_change_walkthrough exactly once as the sole final action.",
            ),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
        ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text(snapshot.prompt()),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
    ];
    let tools = vec![report_tool()];
    let mut usage_span = usage_recorder.begin();
    let request = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::ChangeWalkthrough,
        provider,
        model,
        messages,
        tools,
        options,
    );
    let attempted = async {
        let mut stream = backend
            .stream_prepared(&request)
            .await
            .map_err(|error| error.to_string())?;
        let mut assembler = AssistantTurnAssembler::new();
        while let Some(item) = stream.recv().await {
            let event = item.map_err(|error| error.to_string())?;
            if let crate::agent::stream::ProviderEvent::Usage(usage) = &event {
                usage_span.observe(usage);
            }
            assembler.push(&event).map_err(|error| error.to_string())?;
        }
        assembler.finish().map_err(|error| error.to_string())
    }
    .await;
    let turn = match attempted {
        Ok(turn) => turn,
        Err(error) => {
            usage_span.fail(None);
            return Err(error);
        }
    };
    let usage = turn.usage.clone();
    let validated = (|| {
        if turn.tool_calls.len() != 1 || turn.tool_calls[0].name != "submit_change_walkthrough" {
            return Err(
                "walkthrough response must use submit_change_walkthrough as its sole final call"
                    .into(),
            );
        }
        let result: ChangeWalkthroughResult =
            serde_json::from_str(&turn.tool_calls[0].arguments)
                .map_err(|error| format!("invalid walkthrough report: {error}"))?;
        result.validate(snapshot)
    })();
    match validated {
        Ok(result) => {
            usage_span.complete(usage);
            Ok(result)
        }
        Err(error) => {
            usage_span.fail(usage);
            Err(error)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeWalkthroughStatus {
    Capturing,
    Generating,
    Complete,
    Refused,
    Failed,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeWalkthroughState {
    pub target: ReviewTarget,
    pub provider: String,
    pub model: String,
    pub status: ChangeWalkthroughStatus,
    pub source_hash: Option<String>,
    pub included_files: Vec<String>,
    pub refusal: Option<WalkthroughRefusal>,
    pub result: Option<ChangeWalkthroughResult>,
    pub error: Option<String>,
    pub freshness_error: Option<String>,
    pub scroll: u16,
    pub generation: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::{ChatStreamItem, ToolDef};
    use crate::agent::stream::{FinishReason, ProviderError, ProviderEvent};
    use std::collections::VecDeque;
    use std::path::Path;
    use std::process::Command;
    use std::sync::Mutex;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .unwrap();
        assert!(status.success(), "git failed: {args:?}");
    }

    fn repository() -> TempDir {
        let root = TempDir::new().unwrap();
        git(root.path(), &["init", "-q"]);
        std::fs::write(root.path().join("one.rs"), "fn one() {}\n").unwrap();
        git(root.path(), &["add", "."]);
        git(
            root.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "base",
            ],
        );
        root
    }

    fn git_output(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(output.status.success(), "git failed: {args:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    #[tokio::test]
    async fn complete_snapshot_hash_is_stable_and_changes_with_exact_diff() {
        let root = repository();
        std::fs::write(
            root.path().join("one.rs"),
            "fn one() { println!(\"a\"); }\n",
        )
        .unwrap();
        let service = GitChangeService::new(root.path());
        let first = capture_snapshot(&service, ReviewTarget::Working)
            .await
            .unwrap();
        let second = capture_snapshot(&service, ReviewTarget::Working)
            .await
            .unwrap();
        let (SnapshotCapture::Complete(first), SnapshotCapture::Complete(second)) = (first, second)
        else {
            panic!("expected complete snapshots")
        };
        assert_eq!(first.source_hash, second.source_hash);
        assert_eq!(first.included_files, ["one.rs"]);

        std::fs::write(
            root.path().join("one.rs"),
            "fn one() { println!(\"b\"); }\n",
        )
        .unwrap();
        let SnapshotCapture::Complete(changed) = capture_snapshot(&service, ReviewTarget::Working)
            .await
            .unwrap()
        else {
            panic!("expected changed snapshot")
        };
        assert_ne!(first.source_hash, changed.source_hash);
    }

    #[tokio::test]
    async fn empty_untracked_and_omitted_inputs_refuse_locally() {
        let root = repository();
        let service = GitChangeService::new(root.path());
        let SnapshotCapture::Refused(empty) = capture_snapshot(&service, ReviewTarget::Working)
            .await
            .unwrap()
        else {
            panic!("clean repository must refuse")
        };
        assert!(empty.reason.contains("No textual tracked changes"));

        std::fs::write(root.path().join("new.txt"), "private\n").unwrap();
        let SnapshotCapture::Refused(untracked) = capture_snapshot(&service, ReviewTarget::Working)
            .await
            .unwrap()
        else {
            panic!("untracked input must refuse")
        };
        assert_eq!(untracked.untracked_files, ["new.txt"]);

        let large = format!("{}\n", "x".repeat(crate::changes::MAX_REVIEW_DIFF_BYTES));
        std::fs::write(root.path().join("one.rs"), large).unwrap();
        let SnapshotCapture::Refused(omitted) = capture_snapshot(&service, ReviewTarget::Working)
            .await
            .unwrap()
        else {
            panic!("over-cap input must refuse")
        };
        assert!(omitted.omitted_files.contains(&"one.rs".into()));
    }

    #[tokio::test]
    async fn base_and_commit_targets_capture_exact_committed_ranges() {
        let root = repository();
        let base = git_output(root.path(), &["rev-parse", "HEAD"]);
        std::fs::write(root.path().join("one.rs"), "fn one() { changed(); }\n").unwrap();
        git(root.path(), &["add", "one.rs"]);
        git(
            root.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "change",
            ],
        );
        let commit = git_output(root.path(), &["rev-parse", "HEAD"]);
        let service = GitChangeService::new(root.path());
        let SnapshotCapture::Complete(base_capture) =
            capture_snapshot(&service, ReviewTarget::Base(base.clone()))
                .await
                .unwrap()
        else {
            panic!("base range should be complete")
        };
        let SnapshotCapture::Complete(commit_capture) =
            capture_snapshot(&service, ReviewTarget::Commit(commit.clone()))
                .await
                .unwrap()
        else {
            panic!("commit should be complete")
        };
        assert_eq!(base_capture.included_files, ["one.rs"]);
        assert_eq!(commit_capture.included_files, ["one.rs"]);
        assert_ne!(base_capture.source_hash, commit_capture.source_hash);
        assert!(
            capture_snapshot(&service, ReviewTarget::Base("--unsafe".into()))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn excess_file_count_refuses_and_labels_the_overflow() {
        let root = repository();
        for index in 0..=MAX_WALKTHROUGH_FILES {
            std::fs::write(root.path().join(format!("file-{index:03}.txt")), "base\n").unwrap();
        }
        git(root.path(), &["add", "."]);
        git(
            root.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "many files",
            ],
        );
        for index in 0..=MAX_WALKTHROUGH_FILES {
            std::fs::write(
                root.path().join(format!("file-{index:03}.txt")),
                "changed\n",
            )
            .unwrap();
        }
        let SnapshotCapture::Refused(refusal) =
            capture_snapshot(&GitChangeService::new(root.path()), ReviewTarget::Working)
                .await
                .unwrap()
        else {
            panic!("excess files must refuse")
        };
        assert!(refusal.reason.contains("exceeded"));
        assert!(!refusal.omitted_files.is_empty());
    }

    type FakeTurn = Result<Vec<ChatStreamItem>, ProviderError>;
    type CapturedRequest = (crate::request::RequestKind, Vec<ChatMessage>, Vec<ToolDef>);

    struct FakeBackend {
        turns: Mutex<VecDeque<FakeTurn>>,
        requests: Mutex<Vec<CapturedRequest>>,
    }

    #[async_trait::async_trait]
    impl ChatBackend for FakeBackend {
        async fn stream_chat(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDef],
        ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
            let events = self.turns.lock().unwrap().pop_front().unwrap()?;
            let (tx, rx) = mpsc::unbounded_channel();
            for event in events {
                tx.send(event).unwrap();
            }
            Ok(rx)
        }

        async fn stream_prepared(
            &self,
            request: &crate::request::PreparedModelRequest,
        ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
            self.requests.lock().unwrap().push((
                request.kind,
                request.messages.clone(),
                request.tools.clone(),
            ));
            self.stream_chat(&request.messages, &request.tools).await
        }
    }

    fn report_turn(value: serde_json::Value) -> FakeTurn {
        Ok(vec![
            Ok(ProviderEvent::ToolCallStart {
                index: 0,
                id: "report".into(),
                name: "submit_change_walkthrough".into(),
            }),
            Ok(ProviderEvent::ToolCallArg {
                index: 0,
                arg: value.to_string(),
            }),
            Ok(ProviderEvent::Finish {
                reason: FinishReason::ToolCalls,
                usage: None,
            }),
            Ok(ProviderEvent::Done),
        ])
    }

    fn complete_snapshot() -> ChangeSnapshot {
        let input = ReviewInput {
            scope: "working tree".into(),
            diff: "diff --git a/one.rs b/one.rs\n+println!()".into(),
            included_files: vec!["one.rs".into()],
            omitted_files: Vec::new(),
            untracked_files: Vec::new(),
            custom_instructions: None,
        };
        let SnapshotCapture::Complete(snapshot) =
            snapshot_from_review_input(ReviewTarget::Working, input)
        else {
            panic!("complete fixture")
        };
        assert!(!format!("{snapshot:?}").contains("+println"));
        snapshot
    }

    fn recorder() -> crate::usage::UsageRecorder {
        crate::usage::UsageRecorder::new(
            std::sync::Arc::new(std::sync::Mutex::new(crate::usage::UsageLedger::default())),
            crate::usage::UsageKind::ChangeWalkthrough,
            "provider",
            "model",
        )
    }

    #[tokio::test]
    async fn structured_runner_uses_two_isolated_messages_and_one_report_schema() {
        let backend = FakeBackend {
            turns: Mutex::new(
                vec![report_turn(json!({
                    "summary":"This updates the entry point.",
                    "steps":[{"order":1,"title":"Update output","files":["one.rs"],"explanation":"The changed function now prints."}],
                    "notes":[]
                }))]
                .into(),
            ),
            requests: Mutex::new(Vec::new()),
        };
        let snapshot = complete_snapshot();
        let result = run_walkthrough(
            &backend,
            ("provider", "model"),
            &snapshot,
            crate::agent::client::ChatRequestOptions::default(),
            recorder(),
        )
        .await
        .unwrap();
        assert_eq!(result.steps[0].order, 1);
        let requests = backend.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].0,
            crate::request::RequestKind::ChangeWalkthrough
        );
        assert_eq!(requests[0].1.len(), 2);
        assert_eq!(requests[0].2.len(), 1);
        assert_eq!(requests[0].2[0].function.name, "submit_change_walkthrough");
        let rendered = requests[0].1[1].content.as_str().unwrap();
        assert!(rendered.contains(&snapshot.source_hash));
        assert!(rendered.contains("+println!()"));
        assert!(!rendered.contains("chat history"));
    }

    #[tokio::test]
    async fn report_validation_rejects_nonconsecutive_order_and_unknown_files() {
        for report in [
            json!({"summary":"ok","steps":[{"order":2,"title":"x","files":["one.rs"],"explanation":"y"}],"notes":[]}),
            json!({"summary":"ok","steps":[{"order":1,"title":"x","files":["secret.rs"],"explanation":"y"}],"notes":[]}),
        ] {
            let backend = FakeBackend {
                turns: Mutex::new(vec![report_turn(report)].into()),
                requests: Mutex::new(Vec::new()),
            };
            assert!(run_walkthrough(
                &backend,
                ("provider", "model"),
                &complete_snapshot(),
                crate::agent::client::ChatRequestOptions::default(),
                recorder(),
            )
            .await
            .is_err());
        }
    }

    #[tokio::test]
    async fn text_only_provider_answer_is_rejected() {
        let backend = FakeBackend {
            turns: Mutex::new(
                vec![Ok(vec![
                    Ok(ProviderEvent::Text("free-form answer".into())),
                    Ok(ProviderEvent::Finish {
                        reason: FinishReason::Stop,
                        usage: None,
                    }),
                    Ok(ProviderEvent::Done),
                ])]
                .into(),
            ),
            requests: Mutex::new(Vec::new()),
        };
        let error = run_walkthrough(
            &backend,
            ("provider", "model"),
            &complete_snapshot(),
            crate::agent::client::ChatRequestOptions::default(),
            recorder(),
        )
        .await
        .unwrap_err();
        assert!(error.contains("sole final call"));
    }
}
