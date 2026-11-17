use super::*;
use crate::task_filter::{Policy as TaskPolicy, Preset};

fn fixture() -> (tempfile::TempDir, Arc<Context>) {
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let profile = temporary.path().join("profile");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&profile).unwrap();
    let context = Context::open(profile, workspace, Uuid::new_v4(), None).unwrap();
    context.allow_ephemeral();
    (temporary, context)
}

#[test]
fn task_filter_preview_is_detached_and_new_task_replaces_suggestions() {
    crate::provider_filter::with_test_filter(&Default::default(), || {
        let (_temporary, base) = fixture();
        let first = base
            .with_task_filter(TaskPolicy {
                preset: Preset::PrivateValuesAndSuggestions,
                words: vec!["SecretClient".into()],
                identifiers: vec!["VMX".into()],
            })
            .unwrap();
        let raw = "SecretClient VMX VMXNET3 https://customer.example/api";
        let preview = first.preview_task(raw).unwrap();
        assert!(!preview.provider_copy.contains("SecretClient"));
        assert!(!preview.provider_copy.contains("customer.example"));
        assert!(preview.provider_copy.contains("VMXNET3"));
        assert_eq!(first.restore_text(&preview.provider_copy).unwrap(), raw);
        assert!(base.task_filter().is_none());
        assert!(for_session(base.session_id)
            .unwrap()
            .task_filter()
            .is_none());
        assert!(!crate::provider_filter::process_current().hides_url_hosts());
        let second = first.with_task_filter(TaskPolicy::default()).unwrap();
        let next = second.preview_task(raw).unwrap();
        assert!(next.provider_copy.contains("SecretClient VMX VMXNET3"));
        assert_eq!(second.restore_text(&preview.provider_copy).unwrap(), raw);
        assert!(!first
            .preview_task(raw)
            .unwrap()
            .provider_copy
            .contains("SecretClient"));
    });
}

#[test]
fn task_filter_preview_and_wire_share_aliases_without_double_encoding() {
    crate::provider_filter::with_test_filter(&Default::default(), || {
        let (_temporary, base) = fixture();
        let context = base
            .with_task_filter(TaskPolicy {
                words: vec!["task".into()],
                ..Default::default()
            })
            .unwrap();
        let raw = "task https://private.example/path?q=public";
        let preview = context.preview_task(raw).unwrap();
        let filter = context.task_filter().unwrap();
        let body = serde_json::json!({"messages":[{"role":"user","content":raw}]}).to_string();
        let projected = filter.prepare_request(body.as_bytes()).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&projected).unwrap();
        assert_eq!(value["messages"][0]["content"], preview.provider_copy);
        assert_eq!(filter.prepare_request(&projected).unwrap(), projected);
        assert_eq!(filter.restore_text(&preview.provider_copy), raw);
    });
}

#[test]
fn task_filter_restores_after_private_registry_reload() {
    crate::provider_filter::with_test_filter(&Default::default(), || {
        let (_temporary, base) = fixture();
        let context = base
            .with_task_filter(TaskPolicy {
                words: vec!["saved-private-value".into()],
                ..Default::default()
            })
            .unwrap();
        let raw = "saved-private-value https://saved.example/path";
        let preview = context.preview_task(raw).unwrap();
        let reference = {
            let mut state = context.state.lock().unwrap();
            let record = context.task_filter().unwrap().task_record().unwrap();
            state
                .registry
                .alias("TASK_FILTER", &serde_json::to_string(&record).unwrap())
                .unwrap();
            state.registry.stage(&context.scope).unwrap()
        };
        let registry = AliasRegistry::load(&context.scope, context.session_id, &reference).unwrap();
        let loaded = Context::from_registry(
            context.scope.clone(),
            context.workspace.clone(),
            context.policy_path.clone(),
            CurrentPolicy::new(Policy::default(), false),
            registry,
            Some(reference),
            true,
        )
        .unwrap();
        assert_eq!(loaded.restore_text(&preview.provider_copy).unwrap(), raw);
        assert_eq!(loaded.task_filter_policy(), context.task_filter_policy());
        assert!(loaded.filenames_enabled());
    });
}

#[test]
fn task_filter_remember_records_selected_choices_and_keeps_fresh_suggestions() {
    crate::provider_filter::with_test_filter(&Default::default(), || {
        let (_temporary, base) = fixture();
        let suggested = TaskPolicy {
            words: vec!["malware".into(), "phishing".into()],
            ..Default::default()
        };
        let selected = TaskPolicy {
            words: vec!["malware".into(), "ClientAtlas".into()],
            ..Default::default()
        };
        base.remember_task_selection(&suggested, &selected).unwrap();
        base.remember_task_selection(&suggested, &selected).unwrap();
        let mut fresh = TaskPolicy {
            words: vec!["phishing".into(), "new-topic".into()],
            ..Default::default()
        };
        base.apply_remembered_task_selection(&mut fresh).unwrap();
        assert_eq!(fresh.words, ["new-topic", "malware", "ClientAtlas"]);
        assert!(base.stored_policy().rules.is_empty());
    });
}

#[test]
fn task_filter_preserves_explicit_stop_and_does_not_trip_during_preview() {
    crate::provider_filter::with_test_filter(
        &crate::provider_filter::ProviderFilterConfig {
            enabled: true,
            phrases: vec!["HALT-CANARY".into()],
            ..Default::default()
        },
        || {
            let (_temporary, base) = fixture();
            assert!(base
                .with_task_filter(TaskPolicy {
                    words: vec!["HALT-CANARY".into()],
                    ..Default::default()
                })
                .is_err());
            let context = base.with_task_filter(TaskPolicy::default()).unwrap();
            assert!(context
                .preview_task("HALT-CANARY")
                .unwrap()
                .blocked
                .is_some());
            assert!(!crate::provider_filter::process_current().is_tripped());
            assert!(context
                .task_filter()
                .unwrap()
                .prepare_request(b"HALT-CANARY")
                .is_err());
            assert!(crate::provider_filter::process_current().is_tripped());
        },
    );
}

#[test]
fn task_filter_new_source_reads_use_the_accepted_preset() {
    crate::provider_filter::with_test_filter(&Default::default(), || {
        let (_temporary, base) = fixture();
        let context = base.with_task_filter(TaskPolicy::default()).unwrap();
        let path = context.workspace.join("customer.html");
        let source = "<h1>PRIVATE-CUSTOMER-TITLE</h1><style>h1 { color: red }</style>";
        std::fs::write(&path, source).unwrap();
        let projected = context.project_source("customer.html", source).unwrap();
        assert!(!projected.text.contains("PRIVATE-CUSTOMER-TITLE"));
        assert!(projected.text.contains("color: red"));
        assert_eq!(context.restore_text(&projected.text).unwrap(), source);
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
        assert!(context.task_filter_policy().unwrap().words.is_empty());
    });
}

#[test]
fn task_filter_raw_tool_chunks_and_final_request_keep_long_regex_stops() {
    use crate::provider_filter::{
        ProviderFilterConfig, ProviderFilterKind, ProviderFilterRule, ProviderFilterTarget,
    };
    for boundary in 0..3 {
        crate::provider_filter::with_test_filter(
            &ProviderFilterConfig {
                enabled: true,
                rules: vec![ProviderFilterRule {
                    id: "long-regex".into(),
                    target: ProviderFilterTarget::Request,
                    kind: ProviderFilterKind::Regex,
                    pattern: "BEGIN Client[0-9]+ END".into(),
                }],
                ..Default::default()
            },
            || {
                let (_temporary, base) = fixture();
                let context = base
                    .with_task_filter(TaskPolicy {
                        words: vec!["Client".into()],
                        ..Default::default()
                    })
                    .unwrap();
                let raw = "BEGIN Client123 END";
                assert!(context.preview_task(raw).unwrap().blocked.is_some());
                assert!(!crate::provider_filter::process_current().is_tripped());
                let filter = context.task_filter().unwrap();
                let blocked = match boundary {
                    0 => filter.admit_outbound(raw.as_bytes()).is_err(),
                    1 => {
                        let mut carry = Vec::new();
                        filter
                            .admit_outbound_chunk(&mut carry, b"BEGIN Client")
                            .unwrap();
                        filter.admit_outbound_chunk(&mut carry, b"123 END").is_err()
                    }
                    _ => filter.prepare_request(raw.as_bytes()).is_err(),
                };
                assert!(blocked, "boundary {boundary}");
                assert!(crate::provider_filter::process_current().is_tripped());
            },
        );
    }
}

#[test]
fn task_filter_keeps_explicit_project_default_across_refresh() {
    crate::provider_filter::with_test_filter(&Default::default(), || {
        let (_temporary, base) = fixture();
        let mut stored = base.stored_policy();
        stored.enabled = true;
        stored.default_action = Action::Hide;
        base.save_policy(stored).unwrap();
        let context = base.with_task_filter(TaskPolicy::default()).unwrap();
        assert_eq!(context.policy().default_action, Action::Hide);
        assert!(!context
            .project_source("private.txt", "PRIVATE-CONTENT")
            .unwrap()
            .text
            .contains("PRIVATE-CONTENT"));
        let mut changed = context.stored_policy();
        changed.rules.push(policy::Rule {
            file: "public.txt".into(),
            selector: "**".into(),
            action: Action::Allow,
        });
        context.save_policy(changed).unwrap();
        context.refresh_policy().unwrap();
        assert_eq!(context.policy().default_action, Action::Hide);
        assert_eq!(
            context
                .editor_preview_policy(context.stored_policy())
                .default_action,
            Action::Hide
        );
    });
}

#[test]
fn task_filter_keeps_explicit_page_coder_and_literal_aliases() {
    use crate::provider_filter::{ContentAdjustment, PageStandIn, ProviderFilterConfig};
    crate::provider_filter::with_test_filter(
        &ProviderFilterConfig {
            enabled: true,
            page_stand_in: PageStandIn::Latin,
            adjustments: vec![ContentAdjustment {
                from: "PrivateClient".into(),
                to: "ExistingAlias".into(),
            }],
            ..Default::default()
        },
        || {
            let (_temporary, base) = fixture();
            let context = base.with_task_filter(TaskPolicy::default()).unwrap();
            let filter = context.task_filter().unwrap();
            assert!(filter.substitutes_retrieved());
            assert_eq!(filter.preview_task_text("PrivateClient"), "ExistingAlias");
            let page = filter
                .prepare_page_text("<h1>PrivateClient confidential</h1>")
                .unwrap();
            assert!(!page.contains("confidential"));
            assert_eq!(
                filter.restore_text(&page),
                "<h1>PrivateClient confidential</h1>"
            );
        },
    );
}

#[test]
fn task_filter_word_alias_survives_overlapping_identifier_rewrite() {
    crate::provider_filter::with_test_filter(&Default::default(), || {
        let (_temporary, base) = fixture();
        let context = base
            .with_task_filter(TaskPolicy {
                words: vec!["SecretClient".into()],
                identifiers: vec!["CA".into()],
                ..Default::default()
            })
            .unwrap();
        let raw = "SecretClient CA";
        let preview = context.preview_task(raw).unwrap();
        assert_eq!(context.restore_text(&preview.provider_copy).unwrap(), raw);
        let filter = context.task_filter().unwrap();
        assert_eq!(
            filter.preview_task_text(&preview.provider_copy),
            preview.provider_copy
        );
    });
}

#[test]
fn task_filter_metadata_is_not_an_active_or_restorable_value() {
    crate::provider_filter::with_test_filter(&Default::default(), || {
        let (_temporary, base) = fixture();
        let context = base.with_task_filter(TaskPolicy::default()).unwrap();
        let mut state = context.state.lock().unwrap();
        let entry = state
            .registry
            .entries()
            .iter()
            .find(|entry| entry.kind == "TASK_FILTER")
            .unwrap()
            .clone();
        assert!(state.registry.restore_exact(&entry.token).is_err());
        assert!(state.registry.tokens_in(&entry.token).is_err());
        assert!(!state.active.contains_key(&entry.value));
        let content = state.registry.alias("TEXT", &entry.value).unwrap();
        assert_ne!(content, entry.token);
        assert_eq!(state.registry.restore_exact(&content).unwrap(), entry.value);
    });
}

#[test]
fn task_filter_interleaved_old_commit_keeps_new_policy_and_all_history() {
    crate::provider_filter::with_test_filter(&Default::default(), || {
        let (_temporary, base) = fixture();
        let store = crate::session::SessionStore::new(base.scope.join("sessions"));
        let mut session = crate::session::SessionV4::new(
            "task lineage",
            base.workspace.to_string_lossy(),
            "local",
            "fixture",
            vec![],
        );
        session.id = base.session_id;
        let lease = store.try_writer_lease(session.id).unwrap();
        store.save_owned(&mut session, &lease).unwrap();
        base.set_owner(store.clone(), lease.clone());
        let first = base
            .with_task_filter(TaskPolicy {
                words: vec!["FirstClient".into()],
                ..Default::default()
            })
            .unwrap();
        first.commit_task_filter().unwrap();
        let second_policy = TaskPolicy {
            words: vec!["SecondClient".into()],
            ..Default::default()
        };
        let second = first.with_task_filter(second_policy.clone()).unwrap();
        second.commit_task_filter().unwrap();
        assert!(Arc::ptr_eq(&for_session(session.id).unwrap(), &second));

        let path = base.workspace.join("late.html");
        let source = "<h1>Late private page title</h1>";
        std::fs::write(&path, source).unwrap();
        first.refresh_filenames().unwrap();
        second.refresh_filenames().unwrap();
        let old_path = first.project_filename_path(&path).unwrap();
        let new_path = second.project_filename_path(&path).unwrap();
        let old_source = first
            .project_source("late.html", source)
            .unwrap()
            .text
            .clone();
        let new_source = second
            .project_source("late.html", source)
            .unwrap()
            .text
            .clone();
        let old_url = first
            .preview_task("https://older.example/path")
            .unwrap()
            .provider_copy;
        let new_url = second
            .preview_task("https://newer.example/path")
            .unwrap()
            .provider_copy;
        first.commit_registry().unwrap();
        second.commit_registry().unwrap();
        let saved = store.load_owned(&session.id.to_string(), &lease).unwrap();
        let registry =
            AliasRegistry::load(&base.scope, session.id, saved.privacy.as_ref().unwrap()).unwrap();
        let loaded = Context::from_registry(
            base.scope.clone(),
            base.workspace.clone(),
            base.policy_path.clone(),
            CurrentPolicy::new(Policy::default(), false),
            registry,
            saved.privacy,
            true,
        )
        .unwrap();
        assert_eq!(loaded.task_filter_policy(), Some(second_policy));
        assert_eq!(loaded.restore_text(&old_source).unwrap(), source);
        assert_eq!(loaded.restore_text(&new_source).unwrap(), source);
        assert_eq!(loaded.restore_text(&old_path).unwrap(), "late.html");
        assert_eq!(loaded.restore_text(&new_path).unwrap(), "late.html");
        assert_eq!(
            loaded.restore_text(&old_url).unwrap(),
            "https://older.example/path"
        );
        assert_eq!(
            loaded.restore_text(&new_url).unwrap(),
            "https://newer.example/path"
        );

        second.activate_task_filter();
        let owner = crate::jobs::JobOwner::new(Some(session.id));
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(super::scope(Some(first.clone()), async {
                assert!(Arc::ptr_eq(&super::for_owner(owner).unwrap(), &first));
                assert!(Arc::ptr_eq(&super::for_edit_owner(owner).unwrap(), &first));
            }));
        assert!(Arc::ptr_eq(&super::for_owner(owner).unwrap(), &second));
    });
}

#[test]
fn task_filter_url_snapshot_change_refreshes_without_mutating_active_preview() {
    crate::provider_filter::with_test_filter(&Default::default(), || {
        let (_temporary, base) = fixture();
        let first = base.with_task_filter(TaskPolicy::default()).unwrap();
        first.commit_task_filter().unwrap();
        let second = first.with_task_filter(TaskPolicy::default()).unwrap();
        second
            .preview_task("https://reviewed.example/path")
            .unwrap();
        assert!(first
            .task_filter()
            .unwrap()
            .task_record()
            .unwrap()
            .urls
            .is_empty());
        first
            .preview_task("https://older-worker.example/path")
            .unwrap();
        assert!(second.commit_task_filter().unwrap_err().contains("changed"));
        assert!(Arc::ptr_eq(&for_session(first.session_id).unwrap(), &first));
        assert_eq!(
            first.task_filter().unwrap().task_record().unwrap().urls,
            ["https://older-worker.example"]
        );
    });
}

#[test]
fn task_filter_graph_save_waits_for_private_receipt_publication() {
    crate::provider_filter::with_test_filter(&Default::default(), || {
        use std::sync::mpsc;
        use std::time::Duration;

        let (_temporary, base) = fixture();
        let store = crate::session::SessionStore::new(base.scope.join("sessions"));
        let mut session = crate::session::SessionV4::new(
            "two accepted tasks",
            base.workspace.to_string_lossy(),
            "local",
            "fixture",
            vec![],
        );
        session.id = base.session_id;
        let lease = store.try_writer_lease(session.id).unwrap();
        store.save_owned(&mut session, &lease).unwrap();
        base.set_owner(store.clone(), lease.clone());
        let first = base.with_task_filter(TaskPolicy::default()).unwrap();
        first.commit_task_filter().unwrap();
        let second = first
            .with_task_filter(TaskPolicy {
                words: vec!["SecondClient".into()],
                ..Default::default()
            })
            .unwrap();
        second.commit_task_filter().unwrap();
        let baseline = store.load_owned(&session.id.to_string(), &lease).unwrap();
        let raw = "SecondClient https://late-second-task.example/private";
        let preview = second.preview_task(raw).unwrap();
        let (written_tx, written_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let receipt = second.clone();
        let commit = std::thread::spawn(move || {
            BEFORE_BINDING_PUBLISH.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    written_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                }));
            });
            receipt.commit_registry()
        });
        // Disk already has the new private receipt, but sync_session_binding
        // cannot see its advance until the paused commit publishes it.
        written_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut completed = baseline.work_state.clone();
        completed.agent_graph.status = crate::agent_graph::AgentGraphStatus::Completed;
        let expected = completed.clone();
        let mut persistence = crate::automatic_tasks::GraphPersistence {
            store: store.clone(),
            session: baseline,
            lease: Some(lease.clone()),
        };
        let (started_tx, started_rx) = mpsc::channel();
        let (saved_tx, saved_rx) = mpsc::channel();
        let graph = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            started_tx.send(()).unwrap();
            let result = runtime.block_on(persistence.save(completed));
            saved_tx.send(result.map(|()| persistence.session)).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let early = saved_rx.recv_timeout(Duration::from_millis(100));
        let waited_for_publication = matches!(early, Err(mpsc::RecvTimeoutError::Timeout));
        release_tx.send(()).unwrap();
        commit.join().unwrap().unwrap();
        let saved =
            early.unwrap_or_else(|_| saved_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        graph.join().unwrap();
        assert!(
            waited_for_publication,
            "graph save must not race an unpublished private receipt"
        );
        let saved = saved.unwrap();
        let actual = store.load_owned(&session.id.to_string(), &lease).unwrap();
        assert_eq!(actual.storage_binding, saved.storage_binding);
        assert_eq!(actual.work_state, expected);
        assert_eq!(actual.privacy, saved.privacy);
        assert_eq!(second.restore_text(&preview.provider_copy).unwrap(), raw);

        // Ordering privacy-only receipts must not authorize overwriting a
        // separate local edit with the graph's older metadata baseline.
        let mut changed = actual.clone();
        changed.title = "Intervening local edit".into();
        store.save_owned(&mut changed, &lease).unwrap();
        let mut stale = crate::automatic_tasks::GraphPersistence {
            store: store.clone(),
            session: saved,
            lease: Some(lease.clone()),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert!(runtime.block_on(stale.save(expected)).is_err());
        assert_eq!(
            store
                .load_owned(&session.id.to_string(), &lease)
                .unwrap()
                .title,
            changed.title
        );
    });
}

#[test]
fn task_filter_partial_filename_coverage_stays_local_and_changes_classification() {
    crate::provider_filter::with_test_filter(&Default::default(), || {
        let (_temporary, base) = fixture();
        std::fs::create_dir(base.workspace.join("deep")).unwrap();
        std::fs::write(base.workspace.join("visible.rs"), "source").unwrap();
        let context = base.with_task_filter(TaskPolicy::default()).unwrap();
        context.refresh_filenames().unwrap();
        let complete = context.task_filter_coverage();
        let census =
            filenames::inventory_names_with(&context.workspace, 0, |path| std::fs::read_dir(path))
                .unwrap();
        {
            let mut state = context.state.lock().unwrap();
            assert!(state
                .filenames
                .as_mut()
                .unwrap()
                .apply_census(vec![census])
                .unwrap());
        }
        let partial = context.task_filter_coverage();
        assert!(partial.starts_with("Filename coverage is partial:"));
        assert!(partial.contains("Unscanned bare names may remain readable"));
        assert!(partial.len() < 650);
        let projected = context.mask_known("open visible.rs").unwrap();
        let copy = context.task_filter().unwrap().preview_task_text(&projected);
        assert!(!copy.contains("visible.rs"));
        assert!(!copy.contains("Filename coverage") && !copy.contains("Unscanned"));
        let before_recovery = context.private_revision();
        context.refresh_filenames().unwrap();
        assert!(context.private_revision() > before_recovery);
        assert_eq!(context.task_filter_coverage(), complete);
    });
}
