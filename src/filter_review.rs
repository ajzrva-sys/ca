//! One local, task-owned review at interactive admission. No provider work here.
use crate::app::AppState;
use crate::privacy::Context;
use crate::task_filter::{Policy, Preview};
use crate::tui::privacy::suggestions::{GuideAnalysis, GuideInput, MatchKind};
use crate::{PrivacyEditorResult, StreamMsg};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone)]
struct Input {
    raw: String,
    expanded: String,
    context: Option<String>,
    messages: Vec<crate::app::Message>,
    imported: Option<Arc<crate::provider_history::NativeHistory>>,
    sources: Vec<Arc<crate::attachments::AttachmentSource>>,
    queued: Option<uuid::Uuid>,
}
impl Input {
    fn capture(state: &AppState, raw: &str, expanded: &str, queued: Option<uuid::Uuid>) -> Self {
        Self {
            raw: raw.into(),
            expanded: expanded.into(),
            queued,
            context: state.pending_attachment_context.clone(),
            messages: state.messages.clone(),
            imported: state
                .current_session
                .as_ref()
                .and_then(|s| state.request_store.imported_history(s.id)),
            sources: state.task_filter_attachment_sources.clone(),
        }
    }
    fn guide(&self) -> GuideInput {
        let mut history = Vec::new();
        for message in &self.messages {
            history.push(message.content.clone());
            for call in message.tool_calls.iter().flatten() {
                history.push(call.arguments.clone());
                history.extend(call.output.iter().cloned());
            }
        }
        GuideInput {
            task: self.raw.clone(),
            expanded: self.expanded.clone(),
            history,
            imported: self.imported.clone(),
        }
    }
    fn original(&self) -> String {
        let mut text = format!("Current task\n{}", self.expanded);
        if let Some(context) = &self.context {
            text.push_str("\n\nAttached context\n");
            text.push_str(context);
        }
        for (i, message) in self.messages.iter().enumerate() {
            text.push_str(&format!(
                "\n\nConversation {} · {:?}\n{}",
                i + 1,
                message.role,
                message.content
            ));
            for call in message.tool_calls.iter().flatten() {
                text.push_str(&format!("\n{}\n{}", call.name, call.arguments));
                if let Some(output) = &call.output {
                    text.push('\n');
                    text.push_str(output);
                }
            }
        }
        if let Some(history) = &self.imported {
            for chunk in history.text_chunks() {
                text.push_str("\n\nImported context\n");
                text.push_str(chunk);
            }
        }
        text
    }
    fn fingerprint(&self) -> Result<String, String> {
        let bytes = serde_json::to_vec(&(
            &self.raw,
            &self.expanded,
            &self.context,
            &self.messages,
            &self.sources,
            self.imported
                .as_ref()
                .map(|history| history.text_chunks().collect::<Vec<_>>()),
        ))
        .map_err(|_| "Could not bind the task preview".to_string())?;
        Ok(crate::privacy::digest(&bytes))
    }
    fn sources_unchanged(&self) -> Result<(), String> {
        for source in &self.sources {
            let path = std::path::Path::new(&source.path);
            let same_path = std::fs::canonicalize(path).is_ok_and(|actual| actual == path);
            let same_length = std::fs::metadata(path)
                .is_ok_and(|m| m.is_file() && m.len() == source.source.len() as u64);
            if !same_path || !same_length {
                return Err(
                    "An attached file changed. Review the refreshed task before sending.".into(),
                );
            }
            let actual = std::fs::read(&source.path).map_err(|_| {
                "An attached file is no longer available. Edit the task and attach it again."
                    .to_string()
            })?;
            if actual != source.source.as_bytes() {
                return Err(
                    "An attached file changed. Review the refreshed task before sending.".into(),
                );
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Analyze,
    AnalysisFailed,
    Editing,
    Saving,
    Ready,
    Accepted,
}
#[derive(Clone)]
struct Prepared {
    generation: u64,
    policy: Policy,
    context: Arc<Context>,
    fingerprint: String,
    policy_digest: String,
    blocked: Option<String>,
}
pub(crate) struct Review {
    input: Input,
    phase: Phase,
    busy: bool,
    suggested: Policy,
    defaults: Policy,
    prepared: Option<Prepared>,
}
impl std::fmt::Debug for Review {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskFilterReview")
            .field("busy", &self.busy)
            .finish_non_exhaustive()
    }
}
type SavedTask = (
    Arc<Context>,
    Box<crate::session::SessionV4>,
    Option<crate::session_access::SessionWriterLease>,
);

/// The exact local Send operation, kept separate from worker scheduling so a
/// failed receipt can retry the same reviewed snapshot without provider work.
#[derive(Clone)]
struct SaveTask {
    context: Arc<Context>,
    prepared: Prepared,
    input: Input,
    current: Input,
    remember: Option<Policy>,
    snapshot: crate::session::SessionV4,
    lease: Option<crate::session_access::SessionWriterLease>,
    persist: bool,
    store: crate::session::SessionStore,
}
impl SaveTask {
    fn run(self) -> Result<SavedTask, String> {
        if self.current.fingerprint()? != self.prepared.fingerprint {
            return Err("The conversation changed. Edit the task to refresh its preview.".into());
        }
        self.input.sources_unchanged()?;
        self.context.refresh_policy()?;
        if self.context.policy_fingerprint() != self.prepared.policy_digest {
            return Err("Project rules changed. Edit the task to refresh its preview.".into());
        }
        let candidate = self.prepared.context;
        if let Some(suggested) = self.remember {
            candidate.remember_task_selection(&suggested, &self.prepared.policy)?;
        }
        let mut snapshot = self.snapshot;
        let session_id = snapshot.id;
        let writer = if self.persist {
            let lease = match self.lease {
                Some(lease) => lease,
                None => self
                    .store
                    .try_writer_lease(session_id)
                    .map_err(|e| format!("Could not reserve this task: {e}"))?,
            };
            if let Err(error) = self.store.save_owned(&mut snapshot, &lease) {
                let actual = self
                    .store
                    .load_owned(&session_id.to_string(), &lease)
                    .map_err(|_| format!("Task save failed: {error}"))?;
                if !crate::automatic_tasks::same_saved_state(&actual, &snapshot) {
                    return Err(format!("Task save failed: {error}. Nothing was sent."));
                }
            }
            candidate.set_owner(self.store.clone(), lease.clone());
            let saved = candidate.commit_task_filter().and_then(|()| {
                self.store
                    .load_owned(&session_id.to_string(), &lease)
                    .map_err(|e| format!("Could not read back task filters: {e}"))
            });
            snapshot = match saved {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    // A detached preview must not retain a newly acquired writer
                    // after failure: the identical Send retry needs to acquire it.
                    candidate.clear_owner();
                    return Err(error);
                }
            };
            Some(lease)
        } else {
            candidate.allow_ephemeral();
            candidate.commit_task_filter()?;
            None
        };
        Ok((candidate, Box::new(snapshot), writer))
    }
}

pub(crate) enum ResultMessage {
    Analyzed(Result<(GuideAnalysis, Policy, Policy), String>),
    Previewed {
        generation: u64,
        result: Result<(Arc<Context>, Preview, String, String), String>,
    },
    Saved(Result<SavedTask, String>),
}

pub(crate) fn accepted_queued_input(state: &AppState, raw: &str, id: uuid::Uuid) -> Option<String> {
    state
        .task_filter_review
        .as_ref()
        .filter(|review| {
            review.phase == Phase::Accepted
                && review.input.raw == raw
                && review.input.queued == Some(id)
        })
        .map(|review| review.input.expanded.clone())
}

pub(crate) fn hold(
    state: &mut AppState,
    raw: &str,
    expanded: &str,
    queued: Option<uuid::Uuid>,
) -> bool {
    let _ = (state, raw, expanded, queued);
    false
}

fn send_result(
    tx: mpsc::UnboundedSender<StreamMsg>,
    session_id: uuid::Uuid,
    generation: u64,
    result: ResultMessage,
) {
    let _ = tx.send(StreamMsg::PrivacyEditorFinished {
        session_id,
        generation,
        result: Box::new(PrivacyEditorResult::TaskReview(Box::new(result))),
    });
}

pub(crate) fn dispatch(
    state: &mut AppState,
    tx: &mpsc::UnboundedSender<StreamMsg>,
    store: &crate::session::SessionStore,
) -> Option<(String, String, Option<uuid::Uuid>)> {
    state.task_filter_review.as_ref()?;
    if state
        .privacy_editor
        .as_mut()
        .is_some_and(|e| e.take_disable_preview())
    {
        crate::set_filter_preview_enabled(state, store, false);
        return None;
    }
    if state
        .privacy_editor
        .as_mut()
        .is_some_and(|e| e.take_task_cancel())
    {
        cancel(state, store);
        return None;
    }
    let review = state.task_filter_review.as_ref()?;
    if review.phase == Phase::Ready {
        let review = state.task_filter_review.as_mut()?;
        review.phase = Phase::Accepted;
        state.pending_filter_task = None;
        state.privacy_editor = None;
        state.task_filter_skip_hook = true;
        state.task_filter_send_authorized = state.mode == crate::app::AgentMode::Agent;
        return Some((
            review.input.raw.clone(),
            review.input.expanded.clone(),
            review.input.queued,
        ));
    }
    if review.phase == Phase::Accepted || review.busy {
        return None;
    }
    let session_id = state.current_session.as_ref()?.id;
    let generation = state.privacy_editor_generation;
    let context = state.request_store.privacy_context()?;
    if context.session_id != session_id || state.privacy_binding_in_flight {
        return None;
    }
    if review.phase == Phase::Analyze {
        let input = review.input.clone();
        state.task_filter_review.as_mut()?.busy = true;
        let tx = tx.clone();
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                let mut analysis = input.guide().analyze();
                let mut suggested = Policy::default();
                for term in &analysis.terms {
                    match term.kind {
                        MatchKind::Phrase => suggested.words.push(term.text.clone()),
                        MatchKind::Identifier => {
                            suggested.identifiers.extend(term.spellings.clone())
                        }
                    }
                }
                let original_suggestions = suggested.clone();
                context.apply_remembered_task_selection(&mut suggested)?;
                analysis.terms.retain(|term| match term.kind {
                    MatchKind::Phrase => suggested.words.contains(&term.text),
                    MatchKind::Identifier => term
                        .spellings
                        .iter()
                        .any(|s| suggested.identifiers.contains(s)),
                });
                let mut selected = Policy::default();
                context.apply_remembered_task_selection(&mut selected)?;
                Ok((analysis, selected, original_suggestions))
            })
            .await
            .unwrap_or_else(|_| {
                Err("Local filter analysis stopped. Your task is still held.".into())
            });
            send_result(tx, session_id, generation, ResultMessage::Analyzed(result));
        });
        return None;
    }
    let action = state.privacy_editor.as_mut()?.take_action()?;
    if state.task_filter_review.as_ref()?.phase == Phase::AnalysisFailed {
        if matches!(action, crate::tui::privacy::Action::PreviewTask { .. }) {
            state.task_filter_review.as_mut()?.phase = Phase::Analyze;
        }
        return None;
    }
    match action {
        crate::tui::privacy::Action::PreviewTask {
            policy,
            generation: preview_generation,
        } => {
            let input = state.task_filter_review.as_ref()?.input.clone();
            let workspace = state.workspace.clone();
            state.task_filter_review.as_mut()?.busy = true;
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    context.refresh_policy()?;
                    let policy_digest = context.policy_fingerprint();
                    let candidate = context.with_task_filter(policy)?;
                    candidate.set_tool_roots(&workspace);
                    for source in &input.sources {
                        candidate.record_attachment(source)?;
                    }
                    let preview = candidate.preview_task(&input.original())?;
                    Ok((candidate, preview, input.fingerprint()?, policy_digest))
                })
                .await
                .unwrap_or_else(|_| Err("Local preview stopped. Your task is still held.".into()));
                send_result(
                    tx,
                    session_id,
                    generation,
                    ResultMessage::Previewed {
                        generation: preview_generation,
                        result,
                    },
                );
            });
        }
        crate::tui::privacy::Action::SendTask {
            policy,
            remember,
            generation: preview_generation,
        } => {
            let review = state.task_filter_review.as_ref()?;
            let Some(prepared) = review
                .prepared
                .as_ref()
                .filter(|p| p.generation == preview_generation && p.policy == policy)
            else {
                state
                    .privacy_editor
                    .as_mut()?
                    .apply_guide_failed("Preview changed. Wait for the current comparison.".into());
                return None;
            };
            if let Some(reason) = &prepared.blocked {
                state
                    .privacy_editor
                    .as_mut()?
                    .apply_guide_failed(reason.clone());
                return None;
            }
            if remember && !state.run_capabilities.config_writes {
                state.privacy_editor.as_mut()?.apply_guide_failed("Remember is unavailable in this profile. Clear Remember to send for this task only.".into());
                return None;
            }
            let prepared = prepared.clone();
            let input = review.input.clone();
            let current = Input::capture(state, &input.raw, &input.expanded, input.queued);
            let mut suggested = review.defaults.clone();
            if policy.preset == crate::task_filter::Preset::PrivateValuesAndSuggestions {
                for word in &review.suggested.words {
                    if !suggested.words.contains(word) {
                        suggested.words.push(word.clone());
                    }
                }
                for identifier in &review.suggested.identifiers {
                    if !suggested.identifiers.contains(identifier) {
                        suggested.identifiers.push(identifier.clone());
                    }
                }
            }
            let snapshot = crate::current_session_snapshot(state);
            let lease = state
                .session_writer
                .clone()
                .or_else(|| state.checkpoint_writer.clone());
            let persist = state.run_capabilities.session_persistence;
            state.task_filter_review.as_mut()?.busy = true;
            state.task_filter_review.as_mut()?.phase = Phase::Saving;
            let save = SaveTask {
                context,
                prepared,
                input,
                current,
                remember: remember.then_some(suggested),
                snapshot,
                lease,
                persist,
                store: store.clone(),
            };
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = tokio::task::spawn_blocking(move || save.run())
                    .await
                    .unwrap_or_else(|_| {
                        Err("Saving task filters stopped. Nothing was sent; retry Send.".into())
                    });
                send_result(tx, session_id, generation, ResultMessage::Saved(result));
            });
        }
        crate::tui::privacy::Action::Close => cancel(state, store),
        crate::tui::privacy::Action::DisablePreview => {
            crate::set_filter_preview_enabled(state, store, false);
        }
        _ => {}
    }
    None
}

pub(crate) fn apply(state: &mut AppState, result: ResultMessage) {
    let Some(review) = state.task_filter_review.as_mut() else {
        return;
    };
    review.busy = false;
    match result {
        ResultMessage::Analyzed(Ok((analysis, policy, suggested))) => {
            review.suggested = suggested;
            review.defaults = policy.clone();
            review.phase = Phase::Editing;
            state.privacy_editor = Some(crate::tui::privacy::Editor::task_preview(
                state.cwd.clone(),
                policy,
                analysis,
            ));
        }
        ResultMessage::Analyzed(Err(error)) => {
            review.phase = Phase::AnalysisFailed;
            let analysis = crate::tui::privacy::suggestions::analyze("", "", std::iter::empty());
            let mut editor = crate::tui::privacy::Editor::task_preview(
                state.cwd.clone(),
                Policy::default(),
                analysis,
            );
            let _ = editor.take_action();
            if let Some(generation) = editor.task_preview_generation() {
                editor.apply_task_preview(generation, Err(error));
            }
            state.privacy_editor = Some(editor);
        }
        ResultMessage::Previewed { generation, result } => {
            if state
                .privacy_editor
                .as_ref()
                .and_then(|e| e.task_preview_generation())
                != Some(generation)
            {
                return;
            }
            match result {
                Ok((context, preview, fingerprint, policy_digest)) => {
                    let policy = context.task_filter_policy().unwrap_or_default();
                    review.prepared = Some(Prepared {
                        generation,
                        policy,
                        context,
                        fingerprint,
                        policy_digest,
                        blocked: preview.blocked.clone(),
                    });
                    if let Some(editor) = state.privacy_editor.as_mut() {
                        editor.apply_task_preview(generation, Ok(preview));
                    }
                }
                Err(error) => {
                    review.prepared = None;
                    if let Some(editor) = state.privacy_editor.as_mut() {
                        editor.apply_task_preview(generation, Err(error));
                    }
                }
            }
        }
        ResultMessage::Saved(Ok((context, snapshot, writer))) => {
            // Keep the same owner through the next event-loop tick and planner
            // admission; an idle binding must not clear this committed context.
            state.session_writer = writer;
            context.activate_task_filter();
            state.request_store.set_privacy_context(Some(context));
            state.request_store.set_privacy_pending(false);
            crate::automatic_tasks::adopt_save_baseline(&mut state.current_session, *snapshot);
            review.phase = Phase::Ready;
            state.status_message = Some("Filters ready. Starting your task…".into());
        }
        ResultMessage::Saved(Err(error)) => {
            review.phase = Phase::Editing;
            if error.contains("changed") {
                refresh_changed(state, &error);
            } else if let Some(editor) = state.privacy_editor.as_mut() {
                editor.apply_guide_failed(error);
            }
        }
    }
}

fn refresh_changed(state: &mut AppState, reason: &str) {
    let Some(old) = state.task_filter_review.as_ref().map(|r| r.input.clone()) else {
        return;
    };
    match crate::expand_composer_files(state, &old.raw) {
        Ok(expanded) => {
            let input = Input::capture(state, &old.raw, &expanded, old.queued);
            if let Some(review) = state.task_filter_review.as_mut() {
                review.input = input;
                review.prepared = None;
            }
            state.pending_filter_task = Some((old.raw, expanded, old.queued));
            if let Some(editor) = state.privacy_editor.as_mut() {
                editor.refresh_task_preview();
            }
            state.status_message =
                Some(format!("{reason} Preview refreshed; your edits are kept."));
        }
        Err(error) => {
            if let Some(editor) = state.privacy_editor.as_mut() {
                editor.apply_guide_failed(error);
            }
        }
    }
}

pub(crate) fn cancel_held(state: &mut AppState, store: &crate::session::SessionStore) {
    cancel(state, store);
}

fn cancel(state: &mut AppState, store: &crate::session::SessionStore) {
    let Some(input) = state.task_filter_review.as_ref().map(|r| r.input.clone()) else {
        return;
    };
    if let Some(id) = input.queued {
        if let Some(removed) = state.input_queue.remove_by_id(id) {
            let expected = crate::current_session_snapshot(state);
            if let Err(error) = crate::save_current_session(state, store) {
                let committed = store
                    .load(&expected.id.to_string())
                    .ok()
                    .filter(|actual| crate::automatic_tasks::same_saved_state(actual, &expected));
                if let Some(saved) = committed {
                    crate::automatic_tasks::adopt_save_baseline(&mut state.current_session, saved);
                } else {
                    state.input_queue.push_front(removed);
                    if let Some(editor) = state.privacy_editor.as_mut() {
                        editor.apply_guide_failed(format!(
                            "Could not save cancellation: {error}. Retry Esc; nothing was sent."
                        ));
                    }
                    return;
                }
            }
        }
    }
    state.input = input.raw;
    state.input_cursor = state.input.len();
    state.pending_filter_task = None;
    state.task_filter_review = None;
    state.privacy_editor = None;
    state.task_filter_send_authorized = false;
    state.task_filter_skip_hook = false;
    state.privacy_editor_generation = state.privacy_editor_generation.wrapping_add(1);
    state.status_message = Some("Task restored. Nothing was sent.".into());
}

fn inspection_preview(outbound: crate::privacy::OutboundSnapshot, coverage: String) -> Preview {
    Preview {
        original: (*outbound.original).clone(),
        provider_copy: format!("Latest request\n{}\n\n{}", outbound.latest, outbound.pane),
        changes: outbound.changes.to_string(),
        coverage,
        blocked: None,
        filename_samples: Vec::new(),
    }
}

pub(crate) fn refresh_inspection(state: &mut AppState) {
    if state.task_filter_review.is_some() {
        return;
    }
    let Some(context) = state.request_store.privacy_context() else {
        return;
    };
    let Some(outbound) = context.outbound_snapshot() else {
        return;
    };
    let Some(editor) = state
        .privacy_editor
        .as_mut()
        .filter(|e| e.is_task_preview())
    else {
        return;
    };
    let generation = outbound.generation;
    if editor.task_preview_generation() == Some(generation) {
        return;
    }
    editor.refresh_task_inspection(
        generation,
        inspection_preview(outbound, context.task_filter_coverage()),
    );
}

#[allow(dead_code)]
pub(crate) fn inspect(state: &mut AppState) -> bool {
    let Some(context) = state.request_store.privacy_context() else {
        return false;
    };
    let Some(policy) = context.task_filter_policy() else {
        return false;
    };
    let Some(outbound) = context.outbound_snapshot() else {
        return false;
    };
    state.privacy_editor_generation = state.privacy_editor_generation.wrapping_add(1);
    let generation = outbound.generation;
    let mut editor = crate::tui::privacy::Editor::task_inspection(
        state.cwd.clone(),
        policy,
        inspection_preview(outbound.clone(), context.task_filter_coverage()),
    );
    editor.refresh_task_inspection(
        generation,
        inspection_preview(outbound, context.task_filter_coverage()),
    );
    state.privacy_editor = Some(editor);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> AppState {
        let mut state = AppState::new(crate::context::ShellHistory::in_memory());
        state.task_filter_preview_enabled = true;
        state
    }
    fn prepared_send(root: &std::path::Path, raw: &str) -> (AppState, SaveTask, Preview) {
        let root = std::fs::canonicalize(root).unwrap();
        let store = crate::session::SessionStore::new(root.join("profile/sessions"));
        let mut state = state();
        state.cwd = root.to_string_lossy().into_owned();
        state.workspace =
            crate::workspace::WorkspaceContext::from_primary(root.clone(), root.clone());
        state.session_store = Some(store.clone());
        state.messages.push(crate::app::Message {
            retrieved_spans: Some(Vec::new()),
            role: crate::app::Role::Assistant,
            content: "Previously completed work".into(),
            tool_calls: None,
            thinking: None,
            timestamp: chrono::Utc::now(),
            response_items: Vec::new(),
        });
        let expanded = crate::expand_composer_files(&mut state, raw).unwrap();
        assert!(hold(&mut state, raw, &expanded, None));
        let mut snapshot = crate::current_session_snapshot(&state);
        let lease = store.try_writer_lease(snapshot.id).unwrap();
        store.save_owned(&mut snapshot, &lease).unwrap();
        state.current_session = Some(snapshot.clone());
        drop(lease);
        let context = Context::open(
            store.access_coordinator().scope().to_path_buf(),
            root,
            snapshot.id,
            None,
        )
        .unwrap();
        context.set_tool_roots(&state.workspace);
        state
            .request_store
            .set_privacy_context(Some(context.clone()));
        state.request_store.set_privacy_pending(false);
        let policy = Policy {
            words: vec!["PrivateClient".into()],
            ..Policy::default()
        };
        let analysis =
            crate::tui::privacy::suggestions::analyze(raw, &expanded, std::iter::empty());
        apply(
            &mut state,
            ResultMessage::Analyzed(Ok((analysis, policy.clone(), policy.clone()))),
        );
        let editor = state.privacy_editor.as_mut().unwrap();
        assert!(matches!(
            editor.take_action(),
            Some(crate::tui::privacy::Action::PreviewTask { .. })
        ));
        let generation = editor.task_preview_generation().unwrap();
        let input = state.task_filter_review.as_ref().unwrap().input.clone();
        let candidate = context.with_task_filter(policy).unwrap();
        candidate.set_tool_roots(&state.workspace);
        for source in &input.sources {
            candidate.record_attachment(source).unwrap();
        }
        let preview = candidate.preview_task(&input.original()).unwrap();
        apply(
            &mut state,
            ResultMessage::Previewed {
                generation,
                result: Ok((
                    candidate,
                    preview.clone(),
                    input.fingerprint().unwrap(),
                    context.policy_fingerprint(),
                )),
            },
        );
        let save = SaveTask {
            context,
            prepared: state
                .task_filter_review
                .as_ref()
                .unwrap()
                .prepared
                .clone()
                .unwrap(),
            current: Input::capture(&state, raw, &expanded, None),
            input,
            remember: None,
            snapshot,
            lease: None,
            persist: true,
            store,
        };
        (state, save, preview)
    }

    #[test]
    fn task_filter_admission_same_length_attachment_change_refreshes_before_send() {
        crate::provider_filter::with_test_filter(&Default::default(), || {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("evidence.txt");
            let original = "PrivateClient invoice ALPHA\n";
            let changed = "PrivateClient invoice BRAVO\n";
            assert_eq!(original.len(), changed.len());
            std::fs::write(&path, original).unwrap();
            let (mut state, save, _) = prepared_send(temp.path(), "Inspect @evidence.txt");
            assert_eq!(save.input.sources.len(), 1);
            let generation = state
                .privacy_editor
                .as_ref()
                .unwrap()
                .task_preview_generation()
                .unwrap();
            std::fs::write(&path, changed).unwrap();
            state.task_filter_review.as_mut().unwrap().phase = Phase::Saving;
            state.task_filter_review.as_mut().unwrap().busy = true;
            let result = save.clone().run();
            assert!(result
                .as_ref()
                .err()
                .is_some_and(|error| error.contains("attached file changed")));
            apply(&mut state, ResultMessage::Saved(result));
            let review = state.task_filter_review.as_ref().unwrap();
            assert!(review.phase == Phase::Editing && !review.busy);
            assert!(review.prepared.is_none());
            assert!(review.input.expanded.contains(changed.trim_end()));
            assert_eq!(review.input.sources[0].source, changed);
            assert_eq!(
                state.pending_filter_task.as_ref().unwrap().0,
                "Inspect @evidence.txt"
            );
            let editor = state.privacy_editor.as_mut().unwrap();
            assert!(editor.task_preview_generation().unwrap() > generation);
            assert!(matches!(
                editor.take_action(),
                Some(crate::tui::privacy::Action::PreviewTask { .. })
            ));
            let (tx, _) = mpsc::unbounded_channel();
            assert!(dispatch(&mut state, &tx, &save.store).is_none());
            assert!(!state.task_filter_send_authorized);
            assert!(state.work_state.lock().unwrap().plan_workflow.is_none());
            assert_eq!(state.messages, save.snapshot.messages);
            assert!(save
                .store
                .load(&save.snapshot.id.to_string())
                .unwrap()
                .privacy
                .is_none());
            assert!(save.context.task_filter_policy().is_none());
        });
    }

    #[test]
    fn task_filter_admission_send_save_faults_retry_identical_preview_and_dispatch_once() {
        crate::provider_filter::with_test_filter(&Default::default(), || {
            for point in [
                crate::session::PrivacySaveFault::BeforeCommit,
                crate::session::PrivacySaveFault::AfterCommit,
            ] {
                let temp = tempfile::tempdir().unwrap();
                let raw = "Inspect PrivateClient at https://private.example/task";
                let (mut state, save, preview) = prepared_send(temp.path(), raw);
                let candidate = save.prepared.context.clone();
                assert!(save.lease.is_none());
                assert!(state.session_writer.is_none() && state.checkpoint_writer.is_none());
                // Neither the base context nor the detached candidate retained
                // the fixture's setup writer; this is the first ordinary Send.
                drop(save.store.try_writer_lease(save.snapshot.id).unwrap());
                state.task_filter_review.as_mut().unwrap().phase = Phase::Saving;
                state.task_filter_review.as_mut().unwrap().busy = true;
                let (result, consumed) =
                    crate::session::with_privacy_save_fault(point, || save.clone().run());
                assert!(
                    consumed,
                    "the actual Send receipt must reach the injected failure"
                );
                let (tx, _) = mpsc::unbounded_channel();
                if point == crate::session::PrivacySaveFault::BeforeCommit {
                    assert!(result.is_err());
                    apply(&mut state, ResultMessage::Saved(result));
                    let review = state.task_filter_review.as_ref().unwrap();
                    assert!(review.phase == Phase::Editing && !review.busy);
                    assert!(Arc::ptr_eq(
                        &review.prepared.as_ref().unwrap().context,
                        &candidate
                    ));
                    assert_eq!(state.pending_filter_task.as_ref().unwrap().0, raw);
                    assert!(dispatch(&mut state, &tx, &save.store).is_none());
                    assert!(!state.task_filter_send_authorized);
                    assert!(state.session_writer.is_none() && state.checkpoint_writer.is_none());
                    // The failed candidate receipt released its temporary owner.
                    drop(save.store.try_writer_lease(save.snapshot.id).unwrap());
                    assert!(save
                        .store
                        .load(&save.snapshot.id.to_string())
                        .unwrap()
                        .privacy
                        .is_none());
                    assert!(save.context.task_filter_policy().is_none());
                    assert_eq!(
                        candidate
                            .preview_task(&save.input.original())
                            .unwrap()
                            .provider_copy,
                        preview.provider_copy
                    );
                    state.task_filter_review.as_mut().unwrap().phase = Phase::Saving;
                    state.task_filter_review.as_mut().unwrap().busy = true;
                    apply(&mut state, ResultMessage::Saved(save.clone().run()));
                } else {
                    // Exact readback reconciles an error after the local commit;
                    // it must not ask the user to submit the task a second time.
                    assert!(result.is_ok());
                    apply(&mut state, ResultMessage::Saved(result));
                }
                assert!(state.task_filter_review.as_ref().unwrap().phase == Phase::Ready);
                assert!(!state.task_filter_send_authorized);
                let admitted = dispatch(&mut state, &tx, &save.store).unwrap();
                assert_eq!(admitted.0, raw);
                assert!(state.task_filter_send_authorized);
                assert!(dispatch(&mut state, &tx, &save.store).is_none());
                assert_eq!(state.messages, save.snapshot.messages);
                assert!(state.work_state.lock().unwrap().plan_workflow.is_none());
                let saved = save
                    .store
                    .load_owned(
                        &save.snapshot.id.to_string(),
                        state.session_writer.as_ref().unwrap(),
                    )
                    .unwrap();
                assert_eq!(saved.messages, save.snapshot.messages);
                let resumed = Context::open(
                    save.store.access_coordinator().scope().to_path_buf(),
                    save.context.workspace().to_path_buf(),
                    saved.id,
                    saved.privacy,
                )
                .unwrap();
                assert_eq!(resumed.task_filter_policy(), Some(save.prepared.policy));
                assert_eq!(
                    resumed.restore_text(&preview.provider_copy).unwrap(),
                    preview.original
                );
            }
        });
    }

    #[test]
    fn chatgpt_shim_send_does_not_open_the_filter_preview() {
        let mut state = state();
        state.current_model = Some(crate::app::ModelInfo {
            provider: "chatgpt-shim".into(),
            id: "gpt-5.6-thinking".into(),
            display_name: "thinking".into(),
        });
        assert!(!hold(&mut state, "hello", "hello", None));
        assert!(state.privacy_editor.is_none());
        assert!(state.task_filter_review.is_none());
        assert!(state.pending_filter_task.is_none());

        state.current_model.as_mut().unwrap().provider = "openai-chatgpt".into();
        assert!(!hold(&mut state, "hello", "hello", None));
        assert!(state.privacy_editor.is_none());

        state.current_model.as_mut().unwrap().provider = "deepseek".into();
        assert!(hold(&mut state, "hello", "hello", None));
        assert!(state.privacy_editor.is_some());
    }

    #[test]
    fn task_filter_admission_holds_even_without_suggestions_and_without_a_provider() {
        let mut state = state();
        assert!(hold(&mut state, "hello", "hello", None));
        assert!(state.pending_filter_task.is_some());
        assert!(state.messages.is_empty());
        assert!(state.work_state.lock().unwrap().plan_workflow.is_none());
        assert!(!state.task_filter_send_authorized);
    }
    #[test]
    fn task_filter_admission_cancel_without_context_restores_draft() {
        let temp = tempfile::tempdir().unwrap();
        let store = crate::session::SessionStore::new(temp.path().join("sessions"));
        let mut state = state();
        assert!(hold(
            &mut state,
            "original @draft",
            "expanded contents",
            None
        ));
        let generation = state.privacy_editor_generation;
        state
            .privacy_editor
            .as_mut()
            .unwrap()
            .handle(&crate::tui::Event::Key(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE,
            ));
        let (tx, _) = mpsc::unbounded_channel();
        assert!(dispatch(&mut state, &tx, &store).is_none());
        assert_eq!(state.input, "original @draft");
        assert!(state.task_filter_review.is_none() && state.pending_filter_task.is_none());
        assert!(state.privacy_editor_generation > generation);
        assert!(state.messages.is_empty());
    }
    #[test]
    fn task_filter_admission_acceptance_is_one_use_and_bound_to_input() {
        let mut state = state();
        hold(&mut state, "first", "first", None);
        state.task_filter_review.as_mut().unwrap().phase = Phase::Accepted;
        assert!(!hold(&mut state, "first", "first", None));
        assert!(state.task_filter_review.is_none());
        assert!(hold(&mut state, "next", "next", None));
        assert_eq!(state.task_filter_review.as_ref().unwrap().input.raw, "next");
    }
    #[test]
    fn task_filter_admission_send_retains_writer_through_planner_handoff() {
        crate::provider_filter::with_test_filter(&Default::default(), || {
            let temp = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(temp.path()).unwrap();
            let store = crate::session::SessionStore::new(root.join("profile/sessions"));
            let mut state = state();
            state.cwd = root.to_string_lossy().into_owned();
            state.workspace =
                crate::workspace::WorkspaceContext::from_primary(root.clone(), root.clone());
            state.session_store = Some(store.clone());
            hold(&mut state, "inspect", "inspect", None);
            let mut snapshot = crate::current_session_snapshot(&state);
            let lease = store.try_writer_lease(snapshot.id).unwrap();
            store.save_owned(&mut snapshot, &lease).unwrap();
            let context = Context::open(root.join("profile"), root, snapshot.id, None)
                .unwrap()
                .with_task_filter(Policy::default())
                .unwrap();
            context.set_owner(store.clone(), lease.clone());
            context.commit_task_filter().unwrap();
            snapshot = store.load_owned(&snapshot.id.to_string(), &lease).unwrap();
            apply(
                &mut state,
                ResultMessage::Saved(Ok((context.clone(), Box::new(snapshot), Some(lease)))),
            );
            let (tx, _) = mpsc::unbounded_channel();
            crate::release_idle_privacy_owner(&state);
            assert!(!crate::prepare_privacy_binding(&mut state, &tx));
            context
                .preview_task("https://new-private.example/task")
                .unwrap();
            context.commit_registry().unwrap();
            assert!(state.session_writer.is_some());
            assert!(dispatch(&mut state, &tx, &store).is_some());
            assert!(state.task_filter_send_authorized);
        });
    }
    #[test]
    fn task_filter_admission_failed_analysis_can_retry_or_cancel() {
        let temp = tempfile::tempdir().unwrap();
        let store = crate::session::SessionStore::new(temp.path().join("sessions"));
        let mut state = state();
        hold(&mut state, "task", "task", None);
        apply(
            &mut state,
            ResultMessage::Analyzed(Err("Project filter choices could not be loaded".into())),
        );
        let editor = state.privacy_editor.as_mut().unwrap();
        assert!(editor.is_task_preview());
        editor.handle(&crate::tui::Event::Key(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(matches!(
            editor.take_action(),
            Some(crate::tui::privacy::Action::PreviewTask { .. })
        ));
        editor.handle(&crate::tui::Event::Key(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));
        let (tx, _) = mpsc::unbounded_channel();
        dispatch(&mut state, &tx, &store);
        assert_eq!(state.input, "task");
        assert!(state.task_filter_review.is_none());
    }
    #[test]
    fn task_filter_admission_queued_resume_uses_exact_reviewed_attachment() {
        let mut state = state();
        let id = uuid::Uuid::new_v4();
        hold(
            &mut state,
            "inspect @file",
            "reviewed source bytes",
            Some(id),
        );
        state.task_filter_review.as_mut().unwrap().phase = Phase::Accepted;
        assert_eq!(
            accepted_queued_input(&state, "inspect @file", id).as_deref(),
            Some("reviewed source bytes")
        );
        assert!(accepted_queued_input(&state, "changed @file", id).is_none());
        assert!(accepted_queued_input(&state, "inspect @file", uuid::Uuid::new_v4()).is_none());
    }
}
