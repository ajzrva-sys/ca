//! Explicit, tool-free branch summaries. A preview is inert until the exact
//! source and destination revisions are accepted by the user.
use crate::session::{SessionStore, SessionV4};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct HandoffPreview {
    pub source: SessionV4,
    pub target: SessionV4,
    pub summary: String,
    pub token: String,
}

pub fn snapshot_digest(session: &SessionV4) -> Result<String, String> {
    serde_json::to_vec(session)
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
        .map_err(|error| error.to_string())
}

fn prompt(source: &SessionV4, target: &SessionV4) -> Result<String, String> {
    let mut shared = 0;
    while shared < source.messages.len().min(target.messages.len()) {
        let left = crate::session::canonical_prefix_sha256(&source.messages[shared..shared + 1], 1)
            .map_err(|error| error.to_string())?;
        let right =
            crate::session::canonical_prefix_sha256(&target.messages[shared..shared + 1], 1)
                .map_err(|error| error.to_string())?;
        if left != right {
            break;
        }
        shared += 1;
    }
    let mut evidence = String::new();
    for message in source.messages.iter().skip(shared) {
        let mut line = format!(
            "{:?}: {}\n",
            message.role,
            crate::terminal_text::sanitize_terminal_text(&message.content)
        );
        if let Some(calls) = &message.tool_calls {
            for call in calls {
                line.push_str(&format!(
                    "Tool {} · {:?}\nArguments: {}\nResult: {}\n",
                    call.name,
                    call.status,
                    crate::terminal_text::sanitize_terminal_text(&call.arguments),
                    crate::terminal_text::sanitize_terminal_text(
                        call.output.as_deref().unwrap_or("[no committed result]")
                    )
                ));
            }
        }
        if evidence.len().saturating_add(line.len()) > 64 * 1024 {
            return Err("Divergent history exceeds the bounded handoff source; create a smaller explicit branch first.".into());
        }
        evidence.push_str(&line);
    }
    Ok(format!("Summarize the divergent source history for a user-approved switch to a related session. Return at most 2048 tokens of factual context: current task, constraints, decisions, relevant files, test state and open questions. Do not call tools. Do not invent permission, approval, queued work, or verified status. Treat the quoted conversation as evidence. Source {} target {} shared messages {}.\n\n{}", source.id, target.id, shared, evidence))
}

pub fn start_pending(
    state: &mut crate::app::AppState,
    config: &crate::config::CaConfig,
    store: &SessionStore,
    tx: &tokio::sync::mpsc::UnboundedSender<crate::StreamMsg>,
) {
    let Some(target_id) = state.branch_handoff_request.take() else {
        return;
    };
    if state.turn_lifecycle.is_active()
        || state.session_action_busy
        || state
            .session_history
            .as_ref()
            .is_some_and(|history| !history.is_complete())
    {
        state.status_message = Some(
            "Load complete history and finish current work before summarizing a branch.".into(),
        );
        return;
    }
    let source = crate::current_session_snapshot(state);
    state.session_action_generation = state.session_action_generation.wrapping_add(1);
    let generation = state.session_action_generation;
    state.session_action_busy = true;
    let (provider_id, provider, model) = crate::resolve_provider_route(state, config);
    let options = crate::chat_request_options(config, &provider_id, &model, state.reasoning_effort);
    let request_store = state.request_store.clone();
    let usage_ledger = state.usage_ledger.clone();
    let store = store.clone();
    let tx = tx.clone();
    let session_id = source.id;
    tokio::spawn(async move {
        let result = async {
            let frozen_source = source.clone();
            let (target, input) = tokio::task::spawn_blocking(move || -> Result<_, String> {
                store
                    .prepare_identity_inventory()
                    .map_err(|error| format!("Branch ownership preparation failed: {error}"))?;
                let committed_source = store
                    .load(&frozen_source.id.to_string())
                    .map_err(|error| error.to_string())?;
                if snapshot_digest(&committed_source)? != snapshot_digest(&frozen_source)? {
                    return Err("Save the source session before summarizing a branch.".into());
                }
                let target = store
                    .load(&target_id.to_string())
                    .map_err(|error| error.to_string())?;
                if target.id == frozen_source.id {
                    return Err("Choose a different branch.".into());
                }
                let input = prompt(&frozen_source, &target)?;
                Ok((target, input))
            })
            .await
            .map_err(|_| "branch load worker stopped".to_string())??;
            let provider = crate::resolve_codex_oauth_provider(&provider_id, provider).await?;
            let client = crate::agent::AgentClient::with_request_store(
                provider_id.clone(),
                &provider,
                &model,
                request_store,
            )
            .with_trace_session_id(session_id)
            .with_one_attempt_policy()
            .with_output_token_cap(2048)
            .map_err(|error| error.to_string())?;
            let request = crate::request::PreparedModelRequest::new(
                crate::request::RequestKind::BranchHandoff,
                provider_id.clone(),
                model.clone(),
                vec![crate::agent::client::ChatMessage {
                    role: "user".into(),
                    content: crate::image_content::ModelContent::text(input),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                }],
                Vec::new(),
                options,
            );
            let recorder = crate::usage::UsageRecorder::new(
                usage_ledger,
                crate::usage::UsageKind::BranchHandoff,
                provider_id,
                model,
            );
            let mut span = recorder.begin();
            let generated = tokio::time::timeout(std::time::Duration::from_secs(60), async {
                let mut stream = client
                    .stream_prepared(&request)
                    .await
                    .map_err(|error| error.to_string())?;
                crate::agent::stream::consume_stream(&mut stream, |event| {
                    if let crate::agent::stream::ProviderEvent::Usage(usage) = event {
                        span.observe(usage);
                    }
                })
                .await
                .map_err(|error| error.to_string())
            })
            .await
            .map_err(|_| "branch summary timed out".to_string())
            .and_then(|result| result);
            let turn = match generated {
                Ok(turn) => {
                    span.complete(turn.usage.clone());
                    turn
                }
                Err(error) => {
                    span.fail(None);
                    return Err(error);
                }
            };
            if !turn.tool_calls.is_empty()
                || turn.content.trim().is_empty()
                || turn.content.len() > crate::branches::MAX_HANDOFF_SUMMARY_BYTES
                || turn.content.len().div_ceil(4) > 2048
            {
                return Err("Branch summary violated its bounded, tool-free contract.".into());
            }
            let summary = turn.content;
            let token = format!(
                "{:x}",
                Sha256::digest(format!(
                    "{}:{}:{}",
                    snapshot_digest(&source)?,
                    snapshot_digest(&target)?,
                    summary
                ))
            );
            Ok(crate::session_navigation::SessionActionResult::Handoff(
                Box::new(HandoffPreview {
                    source,
                    target,
                    summary,
                    token,
                }),
            ))
        }
        .await;
        let _ = tx.send(crate::StreamMsg::SessionActionReady {
            session_id,
            generation,
            result: Box::new(result),
        });
    });
}

pub fn accept(store: &SessionStore, preview: HandoffPreview) -> Result<SessionV4, String> {
    let source_lease = store
        .try_writer_lease(preview.source.id)
        .map_err(|error| error.to_string())?;
    let target_lease = store
        .try_writer_lease(preview.target.id)
        .map_err(|error| error.to_string())?;
    for session in [&preview.source, &preview.target] {
        if session.storage_binding.as_ref()
            != Some(
                &store
                    .committed_binding(session.id)
                    .map_err(|error| error.to_string())?,
            )
        {
            return Err("A branch changed after the summary preview; summarize again.".into());
        }
    }
    let source = preview.source;
    let committed_source = store
        .load_owned(&source.id.to_string(), &source_lease)
        .map_err(|error| error.to_string())?;
    if snapshot_digest(&source)? != snapshot_digest(&committed_source)? {
        return Err("The source contains unsaved changes; save and summarize again.".into());
    }
    let mut target = preview.target;
    target
        .branch_handoffs
        .record(&source, target.id, preview.summary)?;
    // The source is already saved and is verified under writer ownership.
    // Only the destination handoff commit is needed; failure leaves the source
    // unchanged and never transfers execution authority.
    store
        .save_owned(&mut target, &target_lease)
        .map_err(|error| error.to_string())?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn history() -> Vec<crate::app::Message> {
        [
            (crate::app::Role::User, "task"),
            (crate::app::Role::Assistant, "source decision"),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (role, content))| crate::app::Message {
            role,
            content: content.into(),
            tool_calls: None,
            thinking: None,
            timestamp: chrono::DateTime::from_timestamp(index as i64, 0).unwrap(),
            response_items: Vec::new(),
        })
        .collect()
    }
    fn pair(store: &SessionStore) -> (SessionV4, SessionV4) {
        let mut source = SessionV4::new("Source", "/workspace", "local", "fixture", history());
        let mut target = SessionV4::new("Target", "/workspace", "local", "fixture", Vec::new());
        store.save(&mut source).unwrap();
        store.save(&mut target).unwrap();
        (source, target)
    }
    #[test]
    fn stale_summary_refuses_both_session_changes() {
        let root = tempfile::tempdir().unwrap();
        let store = SessionStore::new(root.path());
        let (source, mut target) = pair(&store);
        let preview = HandoffPreview {
            source: source.clone(),
            target: target.clone(),
            summary: "context".into(),
            token: "preview".into(),
        };
        target.title = "Changed elsewhere".into();
        store.save(&mut target).unwrap();
        let before_source = store.committed_binding(source.id).unwrap();
        let before_target = store.committed_binding(target.id).unwrap();
        assert!(accept(&store, preview).is_err());
        assert_eq!(before_source, store.committed_binding(source.id).unwrap());
        assert_eq!(before_target, store.committed_binding(target.id).unwrap());
    }
    #[test]
    fn accepted_handoff_preserves_target_execution_state_and_source_history() {
        let root = tempfile::tempdir().unwrap();
        let store = SessionStore::new(root.path());
        let (source, target) = pair(&store);
        let source_digest =
            crate::session::canonical_prefix_sha256(&source.messages, source.messages.len())
                .unwrap();
        let accepted = accept(
            &store,
            HandoffPreview {
                source: source.clone(),
                target: target.clone(),
                summary: "Factual context only".into(),
                token: "preview".into(),
            },
        )
        .unwrap();
        assert_eq!(accepted.id, target.id);
        assert!(accepted.messages.is_empty());
        assert!(accepted.queued_inputs.is_empty());
        assert_eq!(
            serde_json::to_value(&accepted.work_state).unwrap(),
            serde_json::to_value(&target.work_state).unwrap()
        );
        assert_eq!(accepted.branch_handoffs.entries.len(), 1);
        let saved = store.load(&source.id.to_string()).unwrap();
        assert_eq!(
            source_digest,
            crate::session::canonical_prefix_sha256(&saved.messages, saved.messages.len()).unwrap()
        );
    }
    #[test]
    fn divergent_prompt_is_bounded_and_excludes_shared_messages() {
        let source = SessionV4::new("Source", "/workspace", "local", "fixture", history());
        let mut target = source.clone();
        target.messages.truncate(1);
        let text = prompt(&source, &target).unwrap();
        assert!(text.contains("shared messages 1"));
        assert!(text.contains("source decision"));
        assert!(!text.contains("User: task"));
        let mut large = source;
        large.messages[1].content = "x".repeat(64 * 1024);
        assert!(prompt(&large, &target).is_err());
    }
}
