//! Held-out complete-view and newest-content checks for the local publisher.
use super::Context;
use serde_json::{json, Value};
use std::sync::Arc;

fn fixture() -> (tempfile::TempDir, Arc<Context>) {
    let root = tempfile::tempdir().unwrap();
    let context = Context::open(
        root.path().join("profile"),
        root.path().into(),
        uuid::Uuid::new_v4(),
        None,
    )
    .unwrap();
    context.allow_ephemeral();
    let mut policy = context.policy();
    policy.enabled = true;
    context.save_policy(policy).unwrap();
    (root, context)
}

fn responses(history: &[Value], marker: &str, detail: &str) -> Vec<u8> {
    let mut input = history.to_vec();
    input.push(json!({"type":"function_call_output", "call_id":"PUBLIC_CALL_IDENTIFIER", "output":format!("{marker}\n{detail}")}));
    serde_json::to_vec(&json!({"model":"public-model", "input":input, "tools":[]})).unwrap()
}

#[test]
fn privacy_preview_latest_tool_output_precedes_protocol_envelope() {
    let (_root, context) = fixture();
    let history = vec![
        json!({"role":"user", "content":format!("OLDER_HISTORY\n{}\nOLDER_HISTORY_END", "p".repeat(1024 * 1024))}),
    ];
    for marker in ["FIRST_PENDING_MARKER", "SECOND_PENDING_MARKER"] {
        let body = responses(&history, marker, "Public native read result");
        context.publish_wire(&body, &body).unwrap();
        let snapshot = context.outbound_snapshot().unwrap();
        assert!(snapshot.latest.starts_with(marker), "{}", snapshot.latest);
        assert!(snapshot.pane.starts_with(marker), "{}", snapshot.pane);
        assert!(!snapshot.latest.lines().next().unwrap().contains("call_id"));
        assert!(!snapshot
            .latest
            .lines()
            .next()
            .unwrap()
            .contains("function_call_output"));
    }
    assert_eq!(context.outbound_snapshot().unwrap().generation, 2);
}

#[test]
fn privacy_preview_second_update_preserves_complete_sixteen_mib_history_and_local_comparison() {
    let (_root, context) = fixture();
    let padding = "h".repeat((16 * 1024 * 1024_usize).div_ceil(192));
    let history = (0..192)
        .map(|index| {
            json!({
                "role":if index % 2 == 0 { "user" } else { "assistant" },
                "content":format!("HISTORY_BEGIN_{index:03}\n{padding}\nHISTORY_END_{index:03}")
            })
        })
        .collect::<Vec<_>>();
    assert!(serde_json::to_vec(&history).unwrap().len() >= 16 * 1024 * 1024);
    let local = format!(
        "LOCAL_PRIVATE_ORIGINAL_CANARY\n{}\nLOCAL_ORIGINAL_TAIL",
        "private original row\n".repeat(4096)
    );
    let projected = "PROJECTED_REFERENCE_CANARY";
    let original = responses(&history, "FIRST_PENDING_MARKER", &local);
    let sent = responses(&history, "FIRST_PENDING_MARKER", projected);
    context.publish_wire(&original, &sent).unwrap();
    let first = context.outbound_snapshot().unwrap();
    assert!(first.latest.starts_with("FIRST_PENDING_MARKER"));
    let original = responses(&history, "SECOND_PENDING_MARKER", &local);
    let sent = responses(&history, "SECOND_PENDING_MARKER", projected);
    context.publish_wire(&original, &sent).unwrap();
    let second = context.outbound_snapshot().unwrap();
    assert_eq!(second.generation, first.generation + 1);
    assert!(second.latest.starts_with("SECOND_PENDING_MARKER"));
    assert!(second.pane.starts_with("SECOND_PENDING_MARKER"));
    assert!(!second.latest.contains("FIRST_PENDING_MARKER"));
    for (full, detail) in [
        (&second.original, local.as_str()),
        (&second.sent, projected),
    ] {
        let body: Value = serde_json::from_str(full.as_str()).unwrap();
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 193);
        assert!(input[0]["content"]
            .as_str()
            .unwrap()
            .starts_with("HISTORY_BEGIN_000"));
        assert!(input[191]["content"]
            .as_str()
            .unwrap()
            .ends_with("HISTORY_END_191"));
        assert!(input[192]["output"].as_str().unwrap().ends_with(detail));
    }
    assert!(second.original.contains("LOCAL_PRIVATE_ORIGINAL_CANARY"));
    assert!(second.original.contains("LOCAL_ORIGINAL_TAIL"));
    assert!(!second.sent.contains("LOCAL_PRIVATE_ORIGINAL_CANARY"));
    assert!(!second.sent.contains("LOCAL_ORIGINAL_TAIL"));
    assert!(second.sent.contains(projected));
    assert!(second.changes.len() <= 64 * 1024 + 256);
    assert!(second.changes.contains("omitted from Changes"));
    assert!(second
        .changes
        .contains("Full Original/Sent contain the complete request"));
    assert!(second.changes.contains("input[192]"));
}

#[test]
fn privacy_preview_all_opaque_protocol_payloads_stay_out_of_full_and_fallback_views() {
    let (_root, context) = fixture();
    for body in [
        json!({"input":[{"type":"reasoning", "encrypted_content":"OPAQUE_SENTINEL_RESPONSES", "summary":[]}]}),
        json!({"messages":[{"role":"assistant", "content":[
            {"type":"thinking", "thinking":"Public thinking", "signature":"OPAQUE_SENTINEL_ANTHROPIC_SIGNATURE"},
            {"type":"redacted_thinking", "data":"OPAQUE_SENTINEL_ANTHROPIC_DATA", "signature":"OPAQUE_SENTINEL_ANTHROPIC_REDACTED_SIGNATURE"}
        ]}]}),
        json!({"contents":[{"role":"model", "parts":[
            {"thoughtSignature":"OPAQUE_SENTINEL_GOOGLE_SIGNATURE"},
            {"thought_signature":"OPAQUE_SENTINEL_GOOGLE_SNAKE_SIGNATURE", "inlineData":{"mimeType":"image/png", "data":"OPAQUE_SENTINEL_GOOGLE_IMAGE"}},
            {"inline_data":{"mime_type":"image/png", "data":"OPAQUE_SENTINEL_GOOGLE_SNAKE_IMAGE"}}
        ]}]}),
    ] {
        let raw = serde_json::to_vec(&body).unwrap();
        context.publish_wire(&raw, &raw).unwrap();
        let snapshot = context.outbound_snapshot().unwrap();
        assert!(snapshot.opaque_blocks > 0);
        for visible in [
            snapshot.original.as_str(),
            snapshot.sent.as_str(),
            snapshot.latest.as_ref(),
            snapshot.pane.as_ref(),
            snapshot.changes.as_ref(),
        ] {
            assert!(
                !visible.contains("OPAQUE_SENTINEL"),
                "opaque protocol bytes entered a local comparison: {visible}"
            );
        }
        let _: Value = serde_json::from_str(snapshot.original.as_str()).unwrap();
        let _: Value = serde_json::from_str(snapshot.sent.as_str()).unwrap();
        assert!(snapshot.original.contains("opaque provider block"));
        assert!(snapshot.sent.contains("opaque provider block"));
    }
}
