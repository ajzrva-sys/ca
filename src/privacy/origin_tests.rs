use super::*;
use crate::agent::client::AgentClient;
use crate::agent::registry::{ToolContext, ToolProfile};
use crate::app::{Message, Role, ToolCall, ToolStatus};
use crate::image_content::ModelContent;
use serde_json::json;

fn enabled_context(
    store: &crate::session::SessionStore,
    workspace: &Path,
    session: Uuid,
    reference: Option<PrivacySessionRef>,
) -> Arc<Context> {
    let context = Context::open(
        store.access_coordinator().scope().to_path_buf(),
        workspace.into(),
        session,
        reference,
    )
    .unwrap();
    context.allow_ephemeral();
    if !context.enabled() {
        let mut policy = context.policy();
        policy.enabled = true;
        context.save_policy(policy).unwrap();
    }
    context
}

fn assert_distinct_projections(context: &Context, messages: &[Message], private: &str) {
    let wire = AgentClient::build_messages(None, messages, None, &[]).unwrap();
    let tools: Vec<_> = wire
        .iter()
        .filter(|message| message.role == "tool")
        .collect();
    assert_eq!(tools.len(), 2);
    let hidden = tools[0].content.project_privacy(context, true).unwrap();
    let allowed = tools[1].content.project_privacy(context, true).unwrap();
    assert!(!hidden.as_str().unwrap().contains(private));
    assert!(allowed.as_str().unwrap().contains(private));
}

#[tokio::test]
async fn privacy_equal_native_results_keep_distinct_origins_through_session_resume() {
    let temporary = tempfile::tempdir().unwrap();
    let workspace_path = temporary.path().join("workspace");
    std::fs::create_dir(&workspace_path).unwrap();
    let source = "{\"field\":\"identical-private-canary\"}\n";
    for path in ["a.json", "b.json"] {
        std::fs::write(workspace_path.join(path), source).unwrap();
    }
    let store = crate::session::SessionStore::new(temporary.path().join("sessions"));
    let mut session = crate::session::SessionV4::new(
        "origin fixture",
        workspace_path.to_string_lossy(),
        "fixture",
        "fixture",
        vec![],
    );
    let context = enabled_context(&store, &workspace_path, session.id, None);
    let mut policy = context.policy();
    policy.rules.push(policy::Rule {
        file: "b.json".into(),
        selector: "/**".into(),
        action: Action::Allow,
    });
    context.save_policy(policy).unwrap();
    let workspace = crate::workspace::WorkspaceContext::discover(&workspace_path).unwrap();
    let execution_context = ToolContext::native_read(
        &workspace,
        5,
        ToolProfile::Main,
        crate::sandbox::SandboxMode::Off,
        crate::jobs::JobOwner::new(Some(session.id)),
    );
    let requests: Vec<_> = ["a.json", "b.json"]
        .into_iter()
        .map(|path| crate::agent::tools::PreparedNativeRead {
            name: "read_file".into(),
            args: json!({"path":path}),
        })
        .collect();
    let results = scope(
        Some(context.clone()),
        crate::agent::tools::execute_native_read_run(&requests, &execution_context),
    )
    .await;
    assert!(results.iter().all(|result| result.is_success()));
    assert_eq!(results[0].model_text(), results[1].model_text());
    assert_ne!(
        results[0].source_for(results[0].model_text()),
        results[1].source_for(results[1].model_text())
    );
    let calls = results
        .iter()
        .enumerate()
        .map(|(index, result)| ToolCall {
            output_source: result.source_for(result.model_text()),
            id: format!("read-{index}"),
            name: "read_file".into(),
            arguments: requests[index].args.to_string(),
            arguments_retrieved_spans: vec![],
            status: ToolStatus::Done,
            output: Some(result.model_text().into()),
            started_at: None,
        })
        .collect();
    session.messages.push(Message {
        retrieved_spans: Some(vec![]),
        role: Role::Assistant,
        content: String::new(),
        tool_calls: Some(calls),
        thinking: None,
        response_items: vec![],
        timestamp: chrono::Utc::now(),
    });
    assert_distinct_projections(&context, &session.messages, "identical-private-canary");
    session.privacy = Some(
        context
            .state
            .lock()
            .unwrap()
            .registry
            .stage(store.access_coordinator().scope())
            .unwrap(),
    );
    store.save(&mut session).unwrap();
    let id = session.id;
    drop(context);
    let loaded = store.load(&id.to_string()).unwrap();
    assert_eq!(loaded.messages, session.messages);
    let resumed = enabled_context(&store, &workspace_path, id, loaded.privacy.clone());
    assert_distinct_projections(&resumed, &loaded.messages, "identical-private-canary");
}

#[test]
fn privacy_attachment_spans_keep_issued_origin_through_host_wrapping() {
    let temporary = tempfile::tempdir().unwrap();
    let workspace_path = temporary.path().join("workspace");
    std::fs::create_dir(&workspace_path).unwrap();
    std::fs::write(
        workspace_path.join("private.json"),
        "{\"key\":\"attachment-private-canary\"}\n",
    )
    .unwrap();
    let store = crate::session::SessionStore::new(temporary.path().join("sessions"));
    let context = enabled_context(&store, &workspace_path, Uuid::new_v4(), None);
    let workspace = crate::workspace::WorkspaceContext::discover(&workspace_path).unwrap();
    let (text, spans, sources) =
        crate::attachments::expand_file_mentions_with_sources("@private.json", &workspace, &[])
            .unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(
        spans[0].source.as_deref(),
        Some(sources[0].receipt_id.as_str())
    );
    context.record_attachment(&sources[0]).unwrap();
    let wrapped = format!("Review this source.\n{text}");
    let rebased = crate::retrieved_content::rebase_spans(&text, &wrapped, &spans).unwrap();
    assert_eq!(rebased[0].source, spans[0].source);
    let projected = ModelContent::text(wrapped)
        .with_retrieved_spans(rebased)
        .project_privacy(&context, false)
        .unwrap();
    assert!(!projected
        .as_str()
        .unwrap()
        .contains("attachment-private-canary"));
    assert!(projected.as_str().unwrap().contains('{'));
    let rebound: Vec<crate::retrieved_content::RetrievedSpan> =
        serde_json::from_slice(&serde_json::to_vec(&spans).unwrap()).unwrap();
    assert_eq!(rebound, spans);
}

#[test]
fn privacy_matching_text_without_origin_cannot_claim_an_allowed_source() {
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let store = crate::session::SessionStore::new(temporary.path().join("sessions"));
    let context = enabled_context(&store, &workspace, Uuid::new_v4(), None);
    let raw = "{\"field\":\"allowed-only-here\"}";
    let source = context
        .record_read(
            workspace.join("public.json").to_str().unwrap(),
            raw,
            raw,
            1,
            1,
            false,
        )
        .unwrap();
    let mut policy = context.policy();
    policy.rules.push(policy::Rule {
        file: "public.json".into(),
        selector: "/**".into(),
        action: Action::Allow,
    });
    context.save_policy(policy).unwrap();
    assert!(context
        .project_retrieved(raw, Some(&source))
        .unwrap()
        .contains("allowed-only-here"));
    assert!(!context
        .project_retrieved(raw, None)
        .unwrap()
        .contains("allowed-only-here"));
    assert!(context
        .project_retrieved("changed result", Some(&source))
        .is_err());
    assert!(context
        .project_retrieved(raw, Some("unknown-receipt"))
        .is_err());
}

#[test]
fn privacy_unbound_tool_data_cannot_inherit_a_project_wide_allow_rule() {
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let store = crate::session::SessionStore::new(temporary.path().join("sessions"));
    let context = enabled_context(&store, &workspace, Uuid::new_v4(), None);
    let mut policy = context.policy();
    policy.rules.push(policy::Rule {
        file: "**".into(),
        selector: "/**".into(),
        action: Action::Allow,
    });
    context.save_policy(policy).unwrap();
    let output = context
        .project_retrieved("unbound-private-tool-canary", None)
        .unwrap();
    assert!(!output.contains("unbound-private-tool-canary"));
}

#[tokio::test]
async fn privacy_mapped_write_preserves_original_literal_alias_prefix_once() {
    let temporary = tempfile::tempdir().unwrap();
    let workspace_path = temporary.path().join("workspace");
    std::fs::create_dir(&workspace_path).unwrap();
    let store = crate::session::SessionStore::new(temporary.path().join("sessions"));
    let context = enabled_context(&store, &workspace_path, Uuid::new_v4(), None);
    let source = "Literal __CA_PRIVATE_not_a_token appears here.";
    let path = workspace_path.join("literal.txt");
    std::fs::write(&path, source).unwrap();
    let projected = context.project_source("literal.txt", source).unwrap();
    let request = json!({"path":path,"content":format!("{}\npublic addition", projected.text)});
    let prepared = context.prepare_edit_ticket("write_file", &request).unwrap();
    let workspace = crate::workspace::WorkspaceContext::discover(&workspace_path).unwrap();
    let result = scope(
        Some(context.clone()),
        crate::agent::tools::execute_prepared_tool_in_workspace_policy_owned(
            "write_file",
            &prepared,
            &workspace,
            5,
            crate::sandbox::SandboxMode::PolicyOnly,
            false,
            crate::jobs::JobOwner::new(Some(context.session_id)),
        ),
    )
    .await;
    assert!(result.is_success(), "{}", result.model_text());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        format!("{source}\npublic addition")
    );
    context
        .project_source("literal.txt", &std::fs::read_to_string(&path).unwrap())
        .unwrap();
    let unknown = json!({"path":path,"content":"__CA_PRIVATE_0123abcd_TEXT_999__"});
    assert!(context.prepare_edit_ticket("write_file", &unknown).is_err());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        format!("{source}\npublic addition")
    );
}

#[tokio::test]
async fn privacy_mapped_edit_accepts_known_local_calculation_reference_once() {
    let temporary = tempfile::tempdir().unwrap();
    let workspace_path = temporary.path().join("workspace");
    std::fs::create_dir(&workspace_path).unwrap();
    let store = crate::session::SessionStore::new(temporary.path().join("sessions"));
    let context = enabled_context(&store, &workspace_path, Uuid::new_v4(), None);
    let source = "{\"amount\":42}\n";
    let path = workspace_path.join("amount.json");
    std::fs::write(&path, source).unwrap();
    let projected = context.project_source("amount.json", source).unwrap();
    let number = context
        .state
        .lock()
        .unwrap()
        .registry
        .entries()
        .iter()
        .find(|entry| entry.value == "42")
        .unwrap()
        .token
        .clone();
    let calculated = context.project_retrieved("84\n", None).unwrap();
    assert_ne!(calculated.trim(), "84");
    let request = json!({"path":path,"content":projected.text.replace(&number, calculated.trim())});
    let prepared = context.prepare_edit_ticket("write_file", &request).unwrap();
    let workspace = crate::workspace::WorkspaceContext::discover(&workspace_path).unwrap();
    let result = scope(
        Some(context.clone()),
        crate::agent::tools::execute_prepared_tool_in_workspace_policy_owned(
            "write_file",
            &prepared,
            &workspace,
            5,
            crate::sandbox::SandboxMode::PolicyOnly,
            false,
            crate::jobs::JobOwner::new(Some(context.session_id)),
        ),
    )
    .await;
    assert!(result.is_success(), "{}", result.model_text());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"amount\":84}\n");
}

#[test]
fn privacy_edit_view_binds_full_policy_and_rejects_late_same_revision_publication() {
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let store = crate::session::SessionStore::new(temporary.path().join("sessions"));
    let context = enabled_context(&store, &workspace, Uuid::new_v4(), None);
    let path = workspace.join("bound.txt");
    let source = "private original\n";
    std::fs::write(&path, source).unwrap();
    let old_policy = context.policy();
    context.project_source("bound.txt", source).unwrap();
    let arguments = json!({"path":path,"content":"new local value\n"});
    let prepared = context
        .prepare_edit_ticket("write_file", &arguments)
        .unwrap();
    let key = edit_key("write_file", &prepared).unwrap();
    let (old_view, old_ticket) = {
        let state = context.state.lock().unwrap();
        (
            state.views["bound.txt"].clone(),
            state.prepared_edits[&key].clone(),
        )
    };
    // A manual disk replacement can preserve the numeric revision. Model the
    // old worker publishing after refresh cleared the source/ticket bindings.
    let mut replacement = old_policy.clone();
    replacement.rules.push(policy::Rule {
        file: "unused.txt".into(),
        selector: "/**".into(),
        action: Action::Hide,
    });
    std::fs::write(
        &context.policy_path,
        serde_json::to_vec(&replacement).unwrap(),
    )
    .unwrap();
    assert!(context.refresh_policy().unwrap());
    assert_eq!(context.policy().revision, old_policy.revision);
    assert_ne!(context.policy_fingerprint(), old_view.policy_digest);
    {
        let mut state = context.state.lock().unwrap();
        state.views.insert("bound.txt".into(), old_view);
        state.prepared_edits.insert(key, old_ticket);
    }
    assert!(context.translate_edit("write_file", &arguments).is_err());
    assert!(context
        .consume_edit_ticket("write_file", &prepared)
        .is_err());
    context.state.lock().unwrap().views.remove("bound.txt");
    let stale = context.project_source_shared("bound.txt", source, &old_policy, None, true);
    assert!(stale.unwrap_err().contains("Privacy policy changed"));
    assert!(!context
        .state
        .lock()
        .unwrap()
        .views
        .contains_key("bound.txt"));
    // The separately bound stricter policy for unproven tool text still works
    // when current project rules are nonempty.
    assert!(!context
        .project_retrieved("unbound private detail", None)
        .unwrap()
        .contains("unbound private detail"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);
}

#[test]
fn privacy_exact_large_mask_proofs_require_all_bytes_and_current_private_generation() {
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let store = crate::session::SessionStore::new(temporary.path().join("sessions"));
    let context = enabled_context(&store, &workspace, Uuid::new_v4(), None);
    let text = format!(
        "{} NewlyDiscoveredCanary {}",
        "public ".repeat(2000),
        "public ".repeat(2000)
    );
    assert!(context.mask_known_shared(&text).unwrap().is_none());
    assert!(context.mask_known_shared(&text).unwrap().is_none());
    // Same length and same edge samples cannot authorize altered middle bytes.
    let mut changed = text.clone();
    let unknown = "__CA_PRIVATE_0123abcd_TEXT_999__";
    let middle = changed.len() / 2;
    changed.replace_range(middle..middle + unknown.len(), unknown);
    assert_eq!(changed.len(), text.len());
    assert!(context.mask_known_shared(&changed).is_err());
    context
        .project_source("private.json", r#"{"field":"NewlyDiscoveredCanary"}"#)
        .unwrap();
    let replacement = context.mask_known_shared(&text).unwrap().unwrap();
    assert!(!replacement.contains("NewlyDiscoveredCanary"));
    assert!(context.mask_known_shared(&replacement).unwrap().is_none());
    // Revocation drops the proof shortcut; normal inspection still succeeds.
    for proof in &context.state.lock().unwrap().exact_masks {
        proof.ticket.revoke_for_test();
    }
    assert!(context.mask_known_shared(&replacement).unwrap().is_none());
}

#[test]
fn privacy_distinct_default_hide_setup_keeps_first_occurrence_order_and_exact_multiline_restore() {
    for explicit_rule in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let store = crate::session::SessionStore::new(temporary.path().join("sessions"));
        let context = enabled_context(&store, &workspace, Uuid::new_v4(), None);
        if explicit_rule {
            let mut policy = context.policy();
            policy.rules.push(policy::Rule {
                file: "source.rs".into(),
                selector: "/@path".into(),
                action: Action::Allow,
            });
            context.save_policy(policy).unwrap();
        }
        let source = "fn private_fn() {\n    let repeated = r#\"秘密\nprivate inner\"#;\n    println!(\"{}\", repeated);\n    let again = repeated;\n}\n";
        let path = workspace.join("source.rs");
        std::fs::write(&path, source).unwrap();
        let projected = context.project_source("source.rs", source).unwrap();
        assert!(projected
            .changes
            .iter()
            .any(|change| change.original.contains('\n')));
        let expected = view_replacements(&projected);
        {
            let state = context.state.lock().unwrap();
            assert_eq!(state.views["source.rs"].replacements, expected);
            let distinct = projected
                .changes
                .iter()
                .filter(|change| change.action != Action::Allow && !change.original.is_empty())
                .map(|change| (change.original.clone(), change.replacement.clone()))
                .collect::<BTreeMap<_, _>>();
            assert_eq!(state.active, distinct);
            assert!(expected.iter().any(|(reference, original)| {
                original.contains('\n') && reference.ends_with('\n')
            }));
        }
        let prepared = context
            .prepare_edit_ticket(
                "write_file",
                &json!({
                    "path":path,"content":format!("{}\n", projected.text),
                }),
            )
            .unwrap();
        assert_eq!(prepared["content"], format!("{source}\n"));
        context
            .consume_edit_ticket("write_file", &prepared)
            .unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    }
}

#[test]
fn privacy_ephemeral_context_fork_retains_receipts_and_aliases_without_disk_or_edit_state() {
    fn files(path: &Path, root: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        if !path.exists() {
            return;
        }
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files(&path, root, result);
            } else {
                result.insert(
                    path.strip_prefix(root).unwrap().into(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let store = crate::session::SessionStore::new(temporary.path().join("sessions"));
    let parent = enabled_context(&store, &workspace, Uuid::new_v4(), None);
    let path = workspace.join("private.json");
    let source = "{\"field\":\"inherited-private-canary\",\"amount\":42}\n";
    std::fs::write(&path, source).unwrap();
    let raw = render_read(source, 1, 500, false);
    let receipt = parent
        .record_read(&path.to_string_lossy(), source, &raw, 1, 500, false)
        .unwrap();
    let before_projection = parent.project_retrieved(&raw, Some(&receipt)).unwrap();
    let projected = parent.project_source("private.json", source).unwrap();
    parent
        .prepare_edit_ticket("write_file", &json!({"path":path,"content":projected.text}))
        .unwrap();
    parent
        .publish_wire(br#"{"messages":[]}"#, br#"{"messages":[]}"#)
        .unwrap();
    let mut before_files = BTreeMap::new();
    files(temporary.path(), temporary.path(), &mut before_files);
    let child_id = Uuid::new_v4();
    let child = parent.fork_ephemeral(child_id).unwrap();
    assert_eq!(child.session_id, child_id);
    assert_eq!(child.scope, parent.scope);
    assert_eq!(child.workspace, parent.workspace);
    assert_eq!(child.policy(), parent.policy());
    assert!(child.owner.lock().unwrap().is_none());
    assert!(child.ephemeral.load(std::sync::atomic::Ordering::Acquire));
    assert!(child.outbound_snapshot().is_none());
    {
        let state = child.state.lock().unwrap();
        assert!(state.reference.is_none());
        assert!(state.views.is_empty());
        assert!(state.prepared_edits.is_empty());
        assert!(state.cache.is_empty());
        assert!(state.mask_cache.is_empty());
        assert!(state.exact_masks.is_empty());
        assert!(state.advances.is_empty());
        assert!(state.reads.contains_key(&receipt));
        assert_eq!(
            state.registry.entries().len(),
            parent.state.lock().unwrap().registry.entries().len()
        );
    }
    assert_eq!(
        child.project_retrieved(&raw, Some(&receipt)).unwrap(),
        before_projection
    );
    for change in &projected.changes {
        assert_eq!(
            child.restore_text(&change.replacement).unwrap(),
            change.original
        );
    }
    let parent_new = parent
        .project_retrieved("new branch private value", None)
        .unwrap();
    let child_new = child
        .project_retrieved("new branch private value", None)
        .unwrap();
    assert_ne!(parent_new, child_new);
    assert_eq!(
        child.restore_text(&child_new).unwrap(),
        "new branch private value"
    );
    child.commit_registry().unwrap();
    let mut after_files = BTreeMap::new();
    files(temporary.path(), temporary.path(), &mut after_files);
    assert_eq!(after_files, before_files);
    assert!(parent.fork_ephemeral(child_id).is_err());
}
