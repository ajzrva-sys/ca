//! Explicit credentialless measurement through the actual client and HTTP gate.
use super::Context;
use crate::agent::client::{
    AgentClient, ChatMessage, ChatRequestOptions, FunctionCall, ProviderCallPolicy, ToolCallMsg,
    ToolDef, ToolFunction,
};
use crate::agent::stream::{consume_stream, ProviderEvent};
use crate::agent::ProviderProtocol;
use crate::config::{PromptCacheMode, ProviderConfig, ProviderTimeoutConfig};
use crate::image_content::ModelContent;
use crate::request::{PreparedModelRequest, RequestKind, RequestStore};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

const PRIVATE: [&str; 3] = [
    "ClientPerfPrivateKeyCanary",
    "ClientPerfPrivateValueCanary",
    "ClientPerfPrivateOwnerCanary",
];
const PRIVATE_SOURCE: &str = "{\"ClientPerfPrivateKeyCanary\":\"ClientPerfPrivateValueCanary\",\"owner\":\"ClientPerfPrivateOwnerCanary\"}\n";
const WARM_RUNS: usize = 20;

#[derive(Debug)]
struct Capture {
    headers_received: Instant,
    body_received: Instant,
    bytes: usize,
    sha256: String,
    canaries_absent: bool,
    last_marker_present: bool,
}

fn checkpoint(report: &Value) {
    if let Some(path) = std::env::var_os("CA_PRIVACY_CLIENT_PERF_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(report).unwrap()).unwrap();
    }
}

async fn read_request(socket: &mut TcpStream) -> (Vec<u8>, Instant, Instant) {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 64 * 1024];
    let mut head = None;
    loop {
        let count = tokio::time::timeout(Duration::from_secs(30), socket.read(&mut chunk))
            .await
            .unwrap()
            .unwrap();
        assert!(
            count > 0,
            "loopback request closed before its body completed"
        );
        bytes.extend_from_slice(&chunk[..count]);
        assert!(bytes.len() <= 128 * 1024 * 1024);
        if head.is_none() {
            if let Some(end) = bytes
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|end| end + 4)
            {
                let headers_received = Instant::now();
                let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                assert!(headers.starts_with("POST /v1/chat/completions "));
                assert!(!headers.to_ascii_lowercase().contains("authorization:"));
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .expect("actual client sends a complete buffered body");
                head = Some((end, length, headers_received));
            }
        }
        if let Some((end, length, headers_received)) = head {
            if bytes.len() >= end + length {
                let body_received = Instant::now();
                return (
                    bytes[end..end + length].to_vec(),
                    headers_received,
                    body_received,
                );
            }
        }
    }
}

async fn serve(listener: TcpListener, captures: mpsc::UnboundedSender<Capture>) {
    for number in 0..2 * (WARM_RUNS + 1) {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(30), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let (body, headers_received, body_received) = read_request(&mut socket).await;
        let first = json!({"id":"public-performance-response", "object":"chat.completion.chunk", "model":"fixture", "choices":[{"index":0,"delta":{"role":"assistant","content":"PUBLIC_CLIENT_PERF_COMPLETE"},"finish_reason":null}]});
        let last = json!({"id":"public-performance-response", "object":"chat.completion.chunk", "model":"fixture", "choices":[{"index":0,"delta":{},"finish_reason":"stop"}], "usage":{"prompt_tokens":100,"completion_tokens":1,"total_tokens":101}});
        let response = format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n");
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
        let expected_marker = format!("CLIENT_PERF_LAST_MARKER_{}:", number / 2);
        let capture = tokio::task::spawn_blocking(move || {
            let text = std::str::from_utf8(&body).unwrap();
            Capture {
                headers_received,
                body_received,
                bytes: body.len(),
                sha256: super::digest(&body),
                canaries_absent: PRIVATE.iter().all(|value| !text.contains(value)),
                last_marker_present: text.contains(&expected_marker),
            }
        })
        .await
        .unwrap();
        captures.send(capture).unwrap();
    }
}

fn client(endpoint: &str, privacy: Option<Arc<Context>>) -> AgentClient {
    let store = RequestStore::default();
    store.set_privacy_context(privacy);
    let provider = ProviderConfig {
        base_url: endpoint.into(),
        api_key: String::new(),
        protocol: ProviderProtocol::ChatCompletions,
        models: vec!["fixture".into()],
        timeout: ProviderTimeoutConfig::default(),
        google: None,
        tool_schema_policy: Default::default(),
        prompt_cache_mode: PromptCacheMode::Implicit,
    };
    let policy = ProviderCallPolicy {
        connect_timeout: Duration::from_secs(5),
        headers_timeout: Duration::from_secs(30),
        first_event_timeout: Duration::from_secs(30),
        no_progress_timeout: Duration::from_secs(30),
        max_attempts: 1,
        retry_delay: Duration::from_millis(1),
        max_retry_after: Duration::from_millis(1),
    };
    AgentClient::with_call_policy("performance", &provider, "fixture", policy, store)
        .with_filter_for_test(Arc::new(
            crate::provider_filter::ProviderFilter::compile(&Default::default()).unwrap(),
        ))
}

fn message(role: &str, content: impl Into<String>) -> ChatMessage {
    ChatMessage {
        role: role.into(),
        content: ModelContent::text(content),
        tool_calls: None,
        tool_call_id: None,
        response_items: Vec::new(),
    }
}

fn request(target: usize, path: &str, raw: &str, source: &str) -> PreparedModelRequest {
    let mut messages = vec![message(
        "system",
        "Public performance instructions: preserve conversation structure.",
    )];
    let mut remaining = target;
    let public =
        "Previously published public discussion remains available for this ordinary continuation. ";
    let mut index = 0;
    while remaining > 0 {
        let size = remaining.min(16 * 1024);
        let mut text = format!("Public conversation entry {index}: ");
        while text.len() < size {
            text.push_str(public);
        }
        text.truncate(size);
        messages.push(message(
            if index % 2 == 0 { "user" } else { "assistant" },
            text,
        ));
        index += 1;
        remaining -= size;
    }
    let mut assistant = message("assistant", "Public read request.");
    assistant.tool_calls = Some(vec![ToolCallMsg {
        id: "performance-read".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "read_file".into(),
            arguments: json!({"path":path}).to_string(),
            arguments_retrieved_spans: Vec::new(),
        },
    }]);
    messages.push(assistant);
    let mut tool = message("tool", raw);
    tool.tool_call_id = Some("performance-read".into());
    tool.content = tool.content.with_source_receipt(Some(source.into()));
    messages.push(tool);
    messages.push(message(
        "user",
        "CLIENT_PERF_LAST_MARKER_0: continue discussing the public structure.",
    ));
    let tools = vec![ToolDef {
        def_type: "function".into(),
        function: ToolFunction {
            name: "read_file".into(),
            description: "Read public source structure".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
        },
    }];
    let mut request = PreparedModelRequest::new(
        RequestKind::ToolContinuation,
        "performance",
        "fixture",
        messages,
        tools,
        ChatRequestOptions::default(),
    );
    // stream_prepared accepts a prepared harness. Keep process-global guidance
    // out of this disposable fixture while retaining all real client boundaries.
    request.guidance_prepared = true;
    request
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit credentialless actual-client performance measurement; no timing thresholds asserted"]
async fn privacy_actual_client_stream_prepared_loopback_warm_latency() {
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let path = workspace.join("private-input.json");
    std::fs::write(&path, PRIVATE_SOURCE).unwrap();
    let context = Context::open(
        temporary.path().join("profile"),
        workspace,
        uuid::Uuid::new_v4(),
        None,
    )
    .unwrap();
    context.allow_ephemeral();
    let mut policy = context.policy();
    policy.enabled = true;
    context.save_policy(policy).unwrap();
    let raw = super::render_read(PRIVATE_SOURCE, 1, 500, false);
    let source = context
        .record_read(&path.to_string_lossy(), PRIVATE_SOURCE, &raw, 1, 500, false)
        .unwrap();
    let mut report = json!({"schema":"ca.privacy_client_performance.v1", "build":if cfg!(debug_assertions) {"debug"} else {"release"},
        "protocol":"chat_completions", "source_bytes":PRIVATE_SOURCE.len(), "source_sha256":super::digest(PRIVATE_SOURCE.as_bytes()),
        "warm_runs":WARM_RUNS, "sizes":[], "limitations":[
            "Actual AgentClient::stream_prepared path includes original request clones, receipt hashes, adapter, final privacy gate, preview, request snapshots and local HTTP transport.",
            "Start-to-header-arrival includes local connection/header transport and is an upper bound on pre-send client preparation, not an isolated preparation timer.",
            "Body arrival and first text include loopback transfer; no external provider, tokenizer accuracy or model quality is measured.",
            "This disposable no-session path omits durable registry commits; their I/O remains covered by the separate source-stage probe.",
            "Existing isolated preparation budgets are unchanged; this probe adds transport-inclusive evidence and asserts no latency threshold."]});
    checkpoint(&report);
    for target in [1024 * 1024, 16 * 1024 * 1024] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let (tx, mut rx) = mpsc::unbounded_channel();
        let server = tokio::spawn(serve(listener, tx));
        let off = client(&endpoint, None);
        let on = client(&endpoint, Some(context.clone()));
        let mut request = request(target, &path.to_string_lossy(), &raw, &source);
        let message_bytes: usize = request
            .messages
            .iter()
            .filter_map(|message| message.content.as_str())
            .map(str::len)
            .sum();
        let mut samples = Vec::new();
        for index in 0..=WARM_RUNS {
            request.messages.last_mut().unwrap().content = ModelContent::text(format!(
                "CLIENT_PERF_LAST_MARKER_{index}: continue discussing the public structure."
            ));
            let mut pair = Vec::new();
            for (mode, client) in [("off", &off), ("privacy", &on)] {
                let started = Instant::now();
                let mut stream =
                    tokio::time::timeout(Duration::from_secs(30), client.stream_prepared(&request))
                        .await
                        .unwrap()
                        .unwrap();
                let returned = Instant::now();
                let mut first_text = None;
                let turn = consume_stream(&mut stream, |event| {
                    if matches!(event, ProviderEvent::Text(_)) && first_text.is_none() {
                        first_text = Some(Instant::now());
                    }
                })
                .await
                .unwrap();
                let finished = Instant::now();
                assert_eq!(turn.content, "PUBLIC_CLIENT_PERF_COMPLETE");
                let captured = tokio::time::timeout(Duration::from_secs(30), rx.recv())
                    .await
                    .unwrap()
                    .unwrap();
                assert!(captured.last_marker_present);
                assert!(
                    captured.bytes >= target,
                    "actual wire must retain the representative history size"
                );
                assert_eq!(
                    captured.canaries_absent,
                    mode == "privacy",
                    "baseline must retain the same originals while privacy withholds them"
                );
                pair.push(json!({"mode":mode, "wire_bytes":captured.bytes, "wire_sha256":captured.sha256,
                    "raw_canaries_absent":captured.canaries_absent, "last_marker_present":captured.last_marker_present,
                    "client_to_headers_received_seconds":captured.headers_received.duration_since(started).as_secs_f64(),
                    "client_to_body_received_seconds":captured.body_received.duration_since(started).as_secs_f64(),
                    "server_body_transfer_seconds":captured.body_received.duration_since(captured.headers_received).as_secs_f64(),
                    "client_stream_return_seconds":returned.duration_since(started).as_secs_f64(),
                    "client_to_first_text_seconds":first_text.unwrap().duration_since(started).as_secs_f64(),
                    "client_complete_seconds":finished.duration_since(started).as_secs_f64()}));
            }
            samples.push(json!({"run":index + 1, "cache":if index == 0 {"first_request"} else {"warm_continuation"},
                "marker":format!("CLIENT_PERF_LAST_MARKER_{index}"), "off":pair[0], "privacy":pair[1],
                "paired_header_arrival_overhead_seconds":pair[1]["client_to_headers_received_seconds"].as_f64().unwrap()-pair[0]["client_to_headers_received_seconds"].as_f64().unwrap(),
                "paired_body_arrival_overhead_seconds":pair[1]["client_to_body_received_seconds"].as_f64().unwrap()-pair[0]["client_to_body_received_seconds"].as_f64().unwrap()}));
            report["in_progress"] = json!({"target_bytes":target,"source_message_bytes":message_bytes,"message_count":request.messages.len(),"samples":samples});
            checkpoint(&report);
        }
        server.await.unwrap();
        report["sizes"].as_array_mut().unwrap().push(json!({"target_bytes":target,"source_message_bytes":message_bytes,"message_count":request.messages.len(),"samples":samples}));
        checkpoint(&report);
    }
    report["phase"] = json!("complete");
    checkpoint(&report);
    eprintln!(
        "PRV305_CLIENT_PERFORMANCE_JSON={}",
        serde_json::to_string(&report).unwrap()
    );
}
