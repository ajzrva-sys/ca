//! Staged, ignored measurement. Include as a child of `crate::privacy` only.
//! It performs no provider calls and uses only disposable application state.
//! Run explicitly with `--ignored --nocapture --test-threads=1` after source freeze.
use crate::agent::client::{ChatMessage, ChatRequestOptions};
use crate::image_content::ModelContent;
use crate::privacy::{policy::Policy, Context};
use crate::request::{PreparedModelRequest, RequestKind};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Instant;
use uuid::Uuid;

fn context_fixture() -> (tempfile::TempDir, Arc<Context>) {
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let context = Context::open(
        temporary.path().join("profile"),
        workspace,
        Uuid::new_v4(),
        None,
    )
    .unwrap();
    context.allow_ephemeral();
    context
        .save_policy(Policy {
            enabled: true,
            ..context.policy()
        })
        .unwrap();
    (temporary, context)
}

use std::sync::Arc;

#[test]
fn privacy_mask_cache_tracks_new_private_values_policy_and_eviction() {
    let (_temporary, context) = context_fixture();
    let authored = "BeforeSecret LaterSecret remain private";
    assert_eq!(context.mask_known(authored).unwrap(), authored);
    context
        .project_source("before.txt", "BeforeSecret")
        .unwrap();
    let first = context.mask_known(authored).unwrap();
    assert!(!first.contains("BeforeSecret"));
    assert!(first.contains("LaterSecret"));
    context.project_source("later.txt", "LaterSecret").unwrap();
    let both = context.mask_known(authored).unwrap();
    assert!(!both.contains("BeforeSecret"));
    assert!(!both.contains("LaterSecret"));
    for cached in context.state.lock().unwrap().mask_cache.values() {
        cached.ticket.revoke_for_test();
    }
    assert_eq!(context.mask_known(authored).unwrap(), both);
    let mut policy = context.policy();
    policy.rules.push(crate::privacy::policy::Rule {
        file: "*.txt".into(),
        selector: "**".into(),
        action: crate::privacy::policy::Action::Allow,
    });
    context.save_policy(policy).unwrap();
    context
        .project_source("before.txt", "BeforeSecret")
        .unwrap();
    context.project_source("later.txt", "LaterSecret").unwrap();
    assert_eq!(context.mask_known(authored).unwrap(), authored);
}

#[test]
fn privacy_global_masks_keep_structural_punctuation_but_hide_meaningful_values() {
    let (_temporary, context) = context_fixture();
    let source = r#"{"colon":":","slash":"/","space":" ","secret":"!!??","identity":"SensitiveCanary","digits":812739}"#;
    let projected = context.project_source("format.json", source).unwrap();
    assert!(projected
        .changes
        .iter()
        .any(|change| change.original == ":"));
    assert!(projected
        .changes
        .iter()
        .any(|change| change.original == "/"));
    assert!(projected
        .changes
        .iter()
        .any(|change| change.original == " "));
    assert_eq!(context.mask_known("{ : / }  \n").unwrap(), "{ : / }  \n");
    let masked = context.mask_known("SensitiveCanary 812739 !!??").unwrap();
    for value in ["SensitiveCanary", "812739", "!!??"] {
        assert!(!masked.contains(value));
    }
    assert_eq!(context.mask_known(&masked).unwrap(), masked);
}

#[test]
fn privacy_unknown_aliases_are_never_cached_as_public_even_without_patterns() {
    let (_temporary, context) = context_fixture();
    for _ in 0..2 {
        assert!(context
            .mask_known("__CA_PRIVATE_0123abcd_TEXT_999__")
            .is_err());
    }
}

#[test]
fn privacy_wire_bound_accepts_large_history_and_rejects_actual_expanded_limit() {
    let (_temporary, context) = context_fixture();
    let revision = context.policy().revision;
    let fingerprint = context.policy_fingerprint();
    let mut body = br#"{"messages":[]}"#.to_vec();
    body.resize(16 * 1024 * 1024 + 1, b' ');
    assert!(context.validate_wire(&body, revision, &fingerprint).is_ok());
    body.resize(super::MAX_WIRE_BYTES + 1, b' ');
    assert!(context
        .validate_wire(&body, revision, &fingerprint)
        .unwrap_err()
        .contains("128 MiB"));
}

fn elapsed(start: Instant) -> f64 {
    start.elapsed().as_secs_f64()
}

fn checkpoint(report: &Value) {
    if let Some(output) = std::env::var_os("CA_PRIVACY_PERF_OUTPUT") {
        std::fs::write(output, serde_json::to_vec_pretty(report).unwrap()).unwrap();
    }
}

fn disk_usage(path: &Path) -> (u64, u64) {
    let mut paths = vec![path.to_path_buf()];
    let mut files = 0;
    let mut bytes = 0;
    while let Some(path) = paths.pop() {
        let Ok(entries) = std::fs::read_dir(path) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                paths.push(entry.path());
            } else if meta.is_file() {
                files += 1;
                bytes += meta.len();
            }
        }
    }
    (files, bytes)
}

fn message(role: &str, text: String) -> ChatMessage {
    ChatMessage {
        role: role.into(),
        content: ModelContent::text(text),
        tool_calls: None,
        tool_call_id: (role == "tool").then(|| "performance-read".into()),
        response_items: Vec::new(),
    }
}

fn request_for_bytes(target: usize, retrieved: &str) -> PreparedModelRequest {
    let mut messages = vec![message(
        "system",
        "Public performance instructions: retain earlier conversation structure.".into(),
    )];
    let authored_bytes = target.saturating_sub(retrieved.len());
    let base =
        "Previously published public discussion remains available for this ordinary continuation. ";
    let mut remaining = authored_bytes;
    let mut index = 0;
    while remaining > 0 {
        let size = remaining.min(16 * 1024);
        let mut text = format!("Public conversation entry {index}: ");
        while text.len() < size {
            text.push_str(base);
        }
        text.truncate(size);
        messages.push(message(
            if index % 2 == 0 { "user" } else { "assistant" },
            text,
        ));
        remaining -= size;
        index += 1;
    }
    let mut tool = message("tool", retrieved.into());
    tool.content = tool
        .content
        .with_retrieved_spans(crate::retrieved_content::full_span(retrieved));
    messages.push(tool);
    PreparedModelRequest::new(
        RequestKind::ToolContinuation,
        "performance",
        "fixture",
        messages,
        vec![],
        ChatRequestOptions::default(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit disposable performance measurement; reports debug/release timings without performance assertions"]
async fn privacy_representative_source_stage_and_request_latency() {
    let source_path = std::env::var_os("CA_PRIVACY_PERF_SOURCE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"));
    let source = std::fs::read_to_string(source_path).expect("representative source available");
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let scope = temporary.path().join("profile");
    std::fs::create_dir(&workspace).unwrap();
    let path = workspace.join("main.rs");
    std::fs::write(&path, &source).unwrap();
    let context = Context::open(scope.clone(), workspace, Uuid::new_v4(), None).unwrap();
    context.allow_ephemeral();
    context
        .save_policy(Policy {
            enabled: true,
            ..context.policy()
        })
        .unwrap();
    let mut report = json!({"schema":"ca.privacy_performance.internal.v1",
        "build":if cfg!(debug_assertions) { "debug" } else { "release" },
        "source_bytes":source.len(), "source_lines":source.lines().count(),
        "source_sha256":crate::privacy::digest(source.as_bytes()),
        "limitations":["No provider request or model quality measured.","Full-path measurements include actual ChatCompletions adapter, final validation and published local preview; transport/model latency excluded.","No fixed latency threshold is asserted on a debug build."]});
    let worker_context = context.clone();
    let worker_source = source.clone();
    let start = Instant::now();
    let projection = tokio::task::spawn_blocking(move || {
        worker_context.project_source("main.rs", &worker_source)
    })
    .await
    .unwrap()
    .unwrap();
    report["project_source_cold"] = json!({"seconds":elapsed(start), "nodes":projection.nodes.len(),
        "changed_regions":projection.changes.len(),"projection_bytes":projection.text.len()});
    let aliases = context
        .state
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .registry
        .entries()
        .len();
    report["distinct_aliases"] = json!(aliases);
    {
        let state = context.state.lock().unwrap_or_else(|p| p.into_inner());
        report["global_pattern_classes"] = json!({"total":state.active.len(),
            "whitespace_only":state.active.keys().filter(|v| !v.is_empty() && v.chars().all(char::is_whitespace)).count(),
            "one_ascii_punctuation":state.active.keys().filter(|v| v.len() == 1 && v.as_bytes()[0].is_ascii_punctuation()).count(),
            "propagated":state.active.keys().filter(|v| super::globally_private_literal(v)).count()});
    }
    report["phase"] = json!("registry_stage_cold_in_progress");
    checkpoint(&report);
    let start = Instant::now();
    let reference = context
        .state
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .registry
        .stage(&scope)
        .unwrap();
    let stage_seconds = elapsed(start);
    let (files, bytes) = disk_usage(&scope);
    report["registry_stage_cold"] = json!({"seconds":stage_seconds,"entries":reference.entries,"files":files,"disk_bytes":bytes});
    let start = Instant::now();
    context
        .state
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .registry
        .stage(&scope)
        .unwrap();
    report["registry_stage_warm_seconds"] = json!(elapsed(start));
    report["phase"] = json!("warm_projection_in_progress");
    checkpoint(&report);
    let mut warm = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let cached = context.project_source("main.rs", &source).unwrap();
        warm.push(elapsed(start));
        assert_eq!(cached.digest, projection.digest);
    }
    report["project_source_warm_seconds"] = json!(warm);
    checkpoint(&report);
    let worker_context = context.clone();
    let refresh_start = Instant::now();
    let mut refresh = tokio::task::spawn_blocking(move || {
        worker_context.preview_source("main.rs", &worker_context.policy())
    });
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_millis(1));
    let mut responsive_ticks = 0;
    let refreshed = loop {
        tokio::select! {
            result = &mut refresh => break result.unwrap().unwrap(),
            _ = heartbeat.tick() => responsive_ticks += 1,
        }
    };
    report["background_draft_refresh"] = json!({"seconds":elapsed(refresh_start),
        "event_loop_heartbeat_ticks":responsive_ticks,"source_bytes":refreshed.0.len(),
        "projected_bytes":refreshed.1.text.len(),"worker":true});
    assert!(
        responsive_ticks > 0,
        "background refresh must leave the event loop responsive"
    );
    drop(refreshed);
    checkpoint(&report);
    let selected = crate::privacy::render_read(&source, 1, 500, false);
    let source_id = context
        .record_read(&path.to_string_lossy(), &source, &selected, 1, 500, false)
        .unwrap();
    let start = Instant::now();
    let reference = context
        .state
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .registry
        .stage(&scope)
        .unwrap();
    let (files, bytes) = disk_usage(&scope);
    report["registry_stage_read_receipt"] = json!({"seconds":elapsed(start),"entries":reference.entries,"files":files,"disk_bytes":bytes});
    let mut requests = Vec::<Value>::new();
    for size in [1024 * 1024, 16 * 1024 * 1024] {
        let mut request = request_for_bytes(size, &selected);
        if let Some(tool) = request.messages.last_mut() {
            tool.content = tool
                .content
                .clone()
                .with_source_receipt(Some(source_id.clone()));
        }
        let source_bytes: usize = request
            .messages
            .iter()
            .filter_map(|m| m.content.as_str())
            .map(str::len)
            .sum();
        let mut runs = Vec::new();
        let protocol = crate::agent::ProviderProtocol::ChatCompletions;
        let route = crate::agent::stream::ProviderStateSourceV1::for_test("performance", protocol);
        let filter = crate::provider_filter::ProviderFilter::compile(&Default::default()).unwrap();
        for index in 0..21 {
            let mut original = request.clone();
            if let Some(message) = original
                .messages
                .iter_mut()
                .rev()
                .find(|message| message.role == "user")
            {
                message.content = message
                    .content
                    .append_authored(&format!("\nContinuation marker {index}."));
            }
            let off_start = Instant::now();
            let off = protocol
                .adapter()
                .prepare(&original, &route, None, None, None)
                .unwrap();
            let (off_bytes, _) =
                crate::provider_replay::prepare_outbound(&off.body, &off.replay, &filter).unwrap();
            let off_seconds = elapsed(off_start);
            let mut request = original;
            let start = Instant::now();
            let revision = context.prepare_request(&mut request).unwrap();
            let seconds = elapsed(start);
            let projected_bytes: usize = request
                .messages
                .iter()
                .filter_map(|m| m.content.as_str())
                .map(str::len)
                .sum();
            let adapter_start = Instant::now();
            let prepared = protocol
                .adapter()
                .prepare(&request, &route, None, None, None)
                .unwrap();
            let (bytes, _) =
                crate::provider_replay::prepare_outbound(&prepared.body, &prepared.replay, &filter)
                    .unwrap();
            let adapter_seconds = elapsed(adapter_start);
            let validate_start = Instant::now();
            let validation = context.validate_wire(&bytes, revision, &context.policy_fingerprint());
            let validate_seconds = elapsed(validate_start);
            let publish_start = Instant::now();
            let publication = if validation.is_ok() {
                context.publish_wire(&off_bytes, &bytes)
            } else {
                Err("wire gate did not pass".into())
            };
            let publish_seconds = elapsed(publish_start);
            let full_seconds = seconds + adapter_seconds + validate_seconds + publish_seconds;
            runs.push(json!({"run":index + 1,"cache":if index == 0 {"first_request"} else {"warm_continuation"},"seconds":seconds,"projected_bytes":projected_bytes,"policy_revision":revision,
                "off_adapter_seconds":off_seconds,"off_wire_bytes":off_bytes.len(),"adapter_seconds":adapter_seconds,
                "validate_wire_seconds":validate_seconds,"publish_wire_seconds":publish_seconds,"full_seconds":full_seconds,
                "overhead_seconds":full_seconds-off_seconds,"wire_bytes":bytes.len(),"wire_gate_passed":validation.is_ok(),"preview_published":publication.is_ok()}));
            report["request_in_progress"] = json!({"target_bytes":size,"samples":runs});
            checkpoint(&report);
            assert!(
                validation.is_ok(),
                "full wire validation failed: {validation:?}"
            );
            assert!(
                publication.is_ok(),
                "local request preview failed: {publication:?}"
            );
        }
        requests.push(json!({"target_bytes":size,"source_message_bytes":source_bytes,"message_count":request.messages.len(),"samples":runs}));
    }
    report["request_preparation"] = json!(requests);
    report["phase"] = json!("complete");
    if let Some(output) = std::env::var_os("CA_PRIVACY_PERF_OUTPUT") {
        std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    eprintln!(
        "PRV305_PERFORMANCE_JSON={}",
        serde_json::to_string(&report).unwrap()
    );
}
