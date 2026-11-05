use super::*;

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, Arc<Context>) {
    let temporary = tempfile::tempdir().unwrap();
    let scope = temporary.path().join("profile");
    let workspace = temporary.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let context = Context::open(scope.clone(), workspace.clone(), Uuid::new_v4(), None).unwrap();
    context.allow_ephemeral();
    let mut policy = context.policy();
    policy.enabled = true;
    context.save_policy(policy).unwrap();
    (temporary, scope, workspace, context)
}

#[test]
fn privacy_draft_preview_does_not_declassify_or_change_live_bindings() {
    let (_temporary, _scope, workspace, context) = fixture();
    let private = "delicate-client-canary";
    let path = workspace.join("private.txt");
    std::fs::write(&path, private).unwrap();
    context.project_source("private.txt", private).unwrap();
    let (active, entries, cached, view) = {
        let state = context.state.lock().unwrap();
        (
            state.active.clone(),
            state.registry.entries().len(),
            state.cache.len(),
            state.views["private.txt"].source_digest.clone(),
        )
    };
    let committed = context.policy();
    let mut draft = committed.clone();
    draft.rules.push(policy::Rule {
        file: "private.txt".into(),
        selector: "**".into(),
        action: Action::Allow,
    });
    let (_, preview) = context
        .preview_source(path.to_str().unwrap(), &draft)
        .unwrap();
    assert_eq!(preview.text, private);
    assert_eq!(context.policy(), committed);
    assert_ne!(context.mask_known(private).unwrap(), private);
    let state = context.state.lock().unwrap();
    assert_eq!(state.active, active);
    assert_eq!(state.registry.entries().len(), entries);
    assert_eq!(state.cache.len(), cached);
    assert_eq!(state.views["private.txt"].source_digest, view);
}

#[test]
fn privacy_resume_masks_known_values_without_replaying_source_history() {
    let (_temporary, scope, workspace, context) = fixture();
    let private = "resume-private-identity-canary";
    context.project_source("private.txt", private).unwrap();
    let session = context.session_id;
    let reference = context
        .state
        .lock()
        .unwrap()
        .registry
        .stage(&scope)
        .unwrap();
    drop(context);
    let resumed = Context::open(scope, workspace, session, Some(reference)).unwrap();
    assert!(resumed.enabled());
    let authored = format!("Please update {private} next.");
    let sent = resumed.mask_known(&authored).unwrap();
    assert!(!sent.contains(private));
    let raw =
        serde_json::to_vec(&serde_json::json!({"messages": [{"role":"user", "content":authored}]}))
            .unwrap();
    assert!(resumed
        .validate_wire(
            &raw,
            resumed.policy().revision,
            &resumed.policy_fingerprint()
        )
        .is_err());
}

#[test]
fn privacy_multiline_echo_round_trips_two_continuations_without_padding_growth() {
    let (_temporary, _scope, _workspace, context) = fixture();
    let original = "/* delicate first line\ndelicate second line */\nint private_count = 42;\n";
    let first = context.project_source("sample.c", original).unwrap();
    assert!(
        first
            .changes
            .iter()
            .any(|change| change.original.contains('\n')),
        "fixture must contain one real multiline private node"
    );
    let mut sent = first.text.clone();
    for _ in 0..2 {
        let (restored, spans) = context.restore_with_spans(&sent).unwrap();
        assert_eq!(restored, original);
        let content =
            crate::image_content::ModelContent::text(restored).with_retrieved_spans(spans);
        let projected = content.project_privacy(&context, false).unwrap();
        sent = projected.as_str().unwrap().to_string();
        assert_eq!(
            sent.bytes().filter(|byte| *byte == b'\n').count(),
            original.bytes().filter(|byte| *byte == b'\n').count()
        );
    }
    assert_eq!(context.restore_text(&sent).unwrap(), original);
}

#[test]
fn privacy_outbound_snapshot_remains_readable_while_registry_worker_holds_state() {
    let (_temporary, _scope, _workspace, context) = fixture();
    let wire = br#"{"messages":[{"role":"user","content":"prepared request"}]}"#;
    context.publish_wire(wire, wire).unwrap();
    let registry_guard = context.state.lock().unwrap();
    let reader = context.clone();
    let ready = Arc::new(std::sync::Barrier::new(2));
    let reader_ready = ready.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        reader_ready.wait();
        sender
            .send(
                reader
                    .outbound_snapshot()
                    .map(|snapshot| snapshot.generation),
            )
            .unwrap();
    });
    ready.wait();
    let observed = receiver.recv_timeout(std::time::Duration::from_secs(1));
    drop(registry_guard);
    worker.join().unwrap();
    assert_eq!(observed.unwrap(), Some(1));
}

#[tokio::test]
async fn privacy_hashline_broker_formatter_and_native_execution_translate_once() {
    let (_temporary, _scope, workspace, context) = fixture();
    let path = workspace.join("private.rs");
    let source = "const PRIVATE_NAME: &str = \"private-value-canary\";\n";
    std::fs::write(&path, source).unwrap();
    let projected = context.project_source("private.rs", source).unwrap();
    let line = projected.text.lines().next().unwrap();
    let anchor = crate::agent::tools::line_anchor(line);
    let replacement = format!("{line} // reviewed");
    let args = serde_json::json!({"path":path,"start_anchor":anchor,"end_anchor":anchor,"replacement":replacement});
    let workspace =
        crate::workspace::WorkspaceContext::discover(workspace.to_str().unwrap()).unwrap();
    super::scope(Some(context.clone()), async {
        let broker = crate::approval::ApprovalBroker::new(
            &workspace,
            &[],
            crate::approval::ApprovalOrigin::Main,
        );
        let (prepared, _) = broker
            .prepare_formatted(
                "hashline_edit",
                &args,
                crate::agent::registry::ToolOrigin::BuiltIn,
                false,
                crate::sandbox::SandboxMode::Off,
            )
            .await
            .unwrap();
        let result = crate::agent::tools::execute_prepared_tool_in_workspace_policy_owned(
            "hashline_edit",
            &prepared.arguments,
            &workspace,
            5,
            crate::sandbox::SandboxMode::Off,
            false,
            crate::jobs::JobOwner::new(Some(context.session_id)),
        )
        .await;
        assert!(result.is_success(), "{}", result.model_text());
    })
    .await;
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "const PRIVATE_NAME: &str = \"private-value-canary\"; // reviewed\n"
    );
}

#[test]
fn privacy_prepared_edit_cannot_adopt_a_newer_read_view() {
    let (_temporary, _scope, workspace, context) = fixture();
    let path = workspace.join("private.txt");
    std::fs::write(&path, "old-private-value").unwrap();
    let projection = context
        .project_source("private.txt", "old-private-value")
        .unwrap();
    let args = context
        .prepare_edit_ticket(
            "write_file",
            &serde_json::json!({"path":path,"content":projection.text}),
        )
        .unwrap();
    std::fs::write(&path, "new-private-value").unwrap();
    context
        .project_source("private.txt", "new-private-value")
        .unwrap();
    assert!(context.consume_edit_ticket("write_file", &args).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new-private-value");
}

#[test]
fn privacy_cache_hit_rebinds_the_exact_reverted_source_view() {
    let (_temporary, _scope, _workspace, context) = fixture();
    context
        .project_source("private.txt", "first-private-value")
        .unwrap();
    context
        .project_source("private.txt", "second-private-value")
        .unwrap();
    context
        .project_source("private.txt", "first-private-value")
        .unwrap();
    assert_eq!(
        context.state.lock().unwrap().views["private.txt"].source_digest,
        digest(b"first-private-value")
    );
}

#[test]
fn privacy_saved_allow_rule_declassifies_values_without_retained_tool_history() {
    let (_temporary, _scope, workspace, context) = fixture();
    let source = "previously-private-canary";
    let path = workspace.join("private.txt");
    std::fs::write(&path, source).unwrap();
    context
        .record_read(path.to_str().unwrap(), source, source, 1, 1, false)
        .unwrap();
    let mut policy = context.policy();
    policy.rules.push(policy::Rule {
        file: "private.txt".into(),
        selector: "/**".into(),
        action: Action::Allow,
    });
    context.save_policy(policy).unwrap();
    let mut request = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::MainTurn,
        "fixture",
        "fixture",
        vec![crate::agent::client::ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text(source),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        }],
        Vec::new(),
        Default::default(),
    );
    context.prepare_request(&mut request).unwrap();
    assert_eq!(request.messages[0].content.as_str(), Some(source));
}

#[test]
fn privacy_sha256_backend_preserves_durable_identity_vectors() {
    assert_eq!(
        digest(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        digest(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        digest(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}
