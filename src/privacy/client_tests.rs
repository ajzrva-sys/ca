//! Held-out final-transport evidence. Loopback rejects captured requests on
//! purpose; these tests establish protocol/privacy boundaries, not model output.
use super::{policy::Policy, Context};
use crate::agent::client::{
    AgentClient, ChatMessage, FunctionCall, ProviderCallPolicy, ToolCallMsg, ToolDef, ToolFunction,
};
use crate::agent::stream::ProviderError;
use crate::agent::ProviderProtocol;
use crate::config::{PromptCacheMode, ProviderConfig, ProviderTimeoutConfig};
use crate::image_content::ModelContent;
use crate::request::RequestStore;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

const PROTOCOLS: [ProviderProtocol; 4] = [
    ProviderProtocol::ChatCompletions,
    ProviderProtocol::OpenAiResponses,
    ProviderProtocol::AnthropicMessages,
    ProviderProtocol::GoogleGenerateContent,
];
const JSON_SOURCE: &str = "{\n  \"SourceKeyCanary\": \"SourceValueCanary\",\n  \"BudgetKeyCanary\": 728193,\n  \"NestedKeyCanary\": {\"InnerKeyCanary\": \"InnerValueCanary\"}\n}\n";
const MARKDOWN_SOURCE: &str = "# HeadingCanary\n\n- FirstDetailCanary\n- SecondDetailCanary\n";

struct Fixture {
    _root: tempfile::TempDir,
    context: Arc<Context>,
    messages: Vec<ChatMessage>,
    tools: Vec<ToolDef>,
    projected_json: String,
    projected_markdown: String,
}

fn message(role: &str, text: impl Into<String>) -> ChatMessage {
    ChatMessage {
        role: role.into(),
        content: ModelContent::text(text),
        tool_calls: None,
        tool_call_id: None,
        response_items: Vec::new(),
    }
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let context = Context::open(
        root.path().join("profile"),
        workspace.clone(),
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
    let mut messages = vec![
        message(
            "system",
            "PublicInstruction: preserve headings, JSON punctuation, and tool contracts.",
        ),
        message(
            "user",
            "PublicTask: compare SourceValueCanary with FirstDetailCanary.",
        ),
    ];
    let mut assistant = message(
        "assistant",
        "PublicAssistant: read the two supplied sources.",
    );
    let mut calls = Vec::new();
    let mut results = Vec::new();
    for (index, (name, source)) in [
        ("ConfidentialFixtureCanary.json", JSON_SOURCE),
        ("ConfidentialNotesCanary.md", MARKDOWN_SOURCE),
    ]
    .iter()
    .enumerate()
    {
        let path = workspace.join(name);
        std::fs::write(&path, source).unwrap();
        let raw = super::render_read(source, 1, 500, false);
        let source_id = context
            .record_read(&path.to_string_lossy(), source, &raw, 1, 500, false)
            .unwrap();
        let id = format!("privacy-call-{index}");
        calls.push(ToolCallMsg {
            id: id.clone(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: json!({"path": path.to_string_lossy(), "mode":"safe"}).to_string(),
                arguments_retrieved_spans: Vec::new(),
            },
        });
        let spans = crate::retrieved_content::full_span(&raw);
        let mut result = message("tool", raw);
        result.content = result
            .content
            .with_retrieved_spans(spans)
            .with_source_receipt(Some(source_id));
        result.tool_call_id = Some(id);
        results.push(result);
    }
    assistant.tool_calls = Some(calls);
    messages.push(assistant);
    messages.extend(results);
    let projected_json = context
        .project_source("ConfidentialFixtureCanary.json", JSON_SOURCE)
        .unwrap()
        .text
        .clone();
    let projected_markdown = context
        .project_source("ConfidentialNotesCanary.md", MARKDOWN_SOURCE)
        .unwrap()
        .text
        .clone();
    let tools = vec![ToolDef {
        def_type: "function".into(),
        function: ToolFunction {
            name: "read_file".into(),
            description: "PublicToolInstruction".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"},"mode":{"type":"string","enum":["safe"]}},"required":["path"]}),
        },
    }];
    Fixture {
        _root: root,
        context,
        messages,
        tools,
        projected_json,
        projected_markdown,
    }
}

fn client(protocol: ProviderProtocol, endpoint: String, store: RequestStore) -> AgentClient {
    let provider = ProviderConfig {
        base_url: endpoint,
        api_key: String::new(),
        protocol,
        models: vec!["model".into()],
        timeout: ProviderTimeoutConfig::default(),
        google: None,
        tool_schema_policy: Default::default(),
        prompt_cache_mode: PromptCacheMode::Implicit,
    };
    let policy = ProviderCallPolicy {
        connect_timeout: Duration::from_secs(2),
        headers_timeout: Duration::from_secs(3),
        first_event_timeout: Duration::from_secs(2),
        no_progress_timeout: Duration::from_secs(2),
        max_attempts: 1,
        retry_delay: Duration::from_millis(1),
        max_retry_after: Duration::from_millis(1),
    };
    AgentClient::with_call_policy("privacy-test", &provider, "model", policy, store)
        .with_filter_for_test(Arc::new(
            crate::provider_filter::ProviderFilter::compile(&Default::default()).unwrap(),
        ))
}

async fn read_http_body(socket: &mut TcpStream) -> Value {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let count = tokio::time::timeout(Duration::from_secs(3), socket.read(&mut chunk))
            .await
            .unwrap()
            .unwrap();
        assert!(count > 0, "request closed before its complete body");
        request.extend_from_slice(&chunk[..count]);
        assert!(
            request.len() <= 1024 * 1024,
            "fixture request exceeded its bound"
        );
        let Some(header_end) = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|offset| offset + 4)
        else {
            continue;
        };
        let length = String::from_utf8_lossy(&request[..header_end])
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .expect("actual client must supply a bounded content-length");
        if request.len() >= header_end + length {
            return serde_json::from_slice(&request[header_end..header_end + length]).unwrap();
        }
    }
}

fn strings(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(text) => output.push(text.clone()),
        Value::Array(items) => {
            for item in items {
                strings(item, output);
            }
        }
        Value::Object(items) => {
            for item in items.values() {
                strings(item, output);
            }
        }
        _ => {}
    }
}

#[tokio::test]
async fn privacy_final_wire_all_four_protocols_preserves_structure_and_stable_aliases() {
    let fixture = fixture();
    let canonical = serde_json::to_vec(&fixture.messages).unwrap();
    let expected_json = super::render_read(&fixture.projected_json, 1, 500, false);
    let expected_markdown = super::render_read(&fixture.projected_markdown, 1, 500, false);
    for protocol in PROTOCOLS {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let mut bodies = Vec::new();
            for _ in 0..2 {
                let (mut socket, _) =
                    tokio::time::timeout(Duration::from_secs(4), listener.accept())
                        .await
                        .unwrap()
                        .unwrap();
                bodies.push(read_http_body(&mut socket).await);
                // Explicitly reject only after the entire actual wire body was
                // captured. No provider/model completion success is inferred.
                socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
            }
            bodies
        });
        let store = RequestStore::default();
        store.set_privacy_context(Some(fixture.context.clone()));
        let client = client(protocol, endpoint, store);
        let mut messages = fixture.messages.clone();
        for turn in 0..2 {
            if turn == 1 {
                messages.push(message(
                    "user",
                    "PublicSecondTurn: compare SourceValueCanary again.",
                ));
            }
            let result = client.chat_stream(&messages, &fixture.tools).await;
            assert!(
                matches!(result, Err(ProviderError::Http { status: 400, .. })),
                "{protocol:?}: capture fixture must be the only rejection: {result:?}"
            );
        }
        let bodies = server.await.unwrap();
        for body in &bodies {
            let wire = body.to_string();
            for private in [
                "SourceKeyCanary",
                "SourceValueCanary",
                "BudgetKeyCanary",
                "728193",
                "NestedKeyCanary",
                "InnerKeyCanary",
                "InnerValueCanary",
                "HeadingCanary",
                "FirstDetailCanary",
                "SecondDetailCanary",
                "ConfidentialFixtureCanary",
                "ConfidentialNotesCanary",
            ] {
                assert!(!wire.contains(private), "{protocol:?} leaked {private}");
            }
            for internal in [
                "retrieved_spans",
                "source_receipt",
                "manifest_sha256",
                "registry_id",
                "privacy_revision",
                "source_digest",
                "read_receipt",
            ] {
                assert!(
                    !wire.contains(internal),
                    "{protocol:?} transmitted local provenance {internal}"
                );
            }
            assert!(wire.contains("__CA_PRIVATE_"));
            for public in [
                "PublicInstruction",
                "PublicTask",
                "PublicToolInstruction",
                "read_file",
                "path",
                "safe",
            ] {
                assert!(
                    wire.contains(public),
                    "{protocol:?} changed public contract {public}"
                );
            }
            let mut values = Vec::new();
            strings(body, &mut values);
            assert!(
                values.iter().any(|value| value.contains(&expected_json)),
                "{protocol:?} lost exact projected JSON structure"
            );
            assert!(
                values
                    .iter()
                    .any(|value| value.contains(&expected_markdown)),
                "{protocol:?} lost exact projected Markdown structure"
            );
        }
        assert!(bodies[1].to_string().contains("PublicSecondTurn"));
        assert_eq!(
            serde_json::to_vec(&fixture.messages).unwrap(),
            canonical,
            "{protocol:?} changed canonical local history"
        );
    }
}

async fn assert_no_connection(listener: &TcpListener) {
    assert!(
        tokio::time::timeout(Duration::from_millis(80), listener.accept())
            .await
            .is_err(),
        "blocked privacy request opened a provider socket"
    );
}

#[tokio::test]
async fn privacy_known_private_native_schema_fields_block_before_any_socket() {
    let fixture = fixture();
    for protocol in PROTOCOLS {
        for schema in [
            json!({"type":"object","properties":{"SourceKeyCanary":{"type":"string"}}}),
            json!({"type":"object","properties":{"mode":{"type":"string","enum":["SourceValueCanary"]}}}),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let store = RequestStore::default();
            store.set_privacy_context(Some(fixture.context.clone()));
            let client = client(
                protocol,
                format!("http://{}/v1", listener.local_addr().unwrap()),
                store,
            );
            let tools = [ToolDef {
                def_type: "function".into(),
                function: ToolFunction {
                    name: "private_contract".into(),
                    description: "PublicSchemaInstruction".into(),
                    parameters: schema,
                },
            }];
            let result = client.chat_stream(&fixture.messages, &tools).await;
            assert!(
                matches!(result, Err(ProviderError::PrivacyBlocked { .. })),
                "{protocol:?}: schema must be withheld locally: {result:?}"
            );
            assert_no_connection(&listener).await;
        }
    }
}

#[tokio::test]
async fn privacy_pending_guard_blocks_all_protocols_before_any_socket() {
    for protocol in PROTOCOLS {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let store = RequestStore::default();
        store.set_privacy_pending(true);
        let client = client(
            protocol,
            format!("http://{}/v1", listener.local_addr().unwrap()),
            store,
        );
        let result = client
            .chat_stream(&[message("user", "PublicPendingGuard")], &[])
            .await;
        assert!(
            matches!(result, Err(ProviderError::PrivacyBlocked { .. })),
            "{protocol:?}: pending guard must withhold request: {result:?}"
        );
        assert_no_connection(&listener).await;
    }
}

#[test]
fn privacy_final_wire_gate_rejects_stale_policy_revision() {
    let fixture = fixture();
    let revision = fixture.context.policy().revision;
    fixture
        .context
        .save_policy(fixture.context.policy())
        .unwrap();
    let result = fixture.context.validate_wire(
        br#"{"messages":[]}"#,
        revision,
        &fixture.context.policy_fingerprint(),
    );
    assert!(result.unwrap_err().contains("changed before transmission"));
}

fn opaque_item(
    protocol: ProviderProtocol,
    source: crate::agent::stream::ProviderStateSourceV1,
) -> crate::agent::stream::ProviderResponseItem {
    use crate::agent::stream::ProviderResponseItem;
    match protocol {
        ProviderProtocol::OpenAiResponses => ProviderResponseItem::ResponsesOutput {
            privacy_prefix_sha256: None,
            source,
            output_index: 0,
            item: json!({"id":"opaque-test-reasoning","type":"reasoning","encrypted_content":"opaque-replay-sentinel","summary":[]}),
        },
        ProviderProtocol::AnthropicMessages => ProviderResponseItem::NativeContent {
            source,
            model: "model".into(),
            prefix_sha256: "0".repeat(64),
            content: vec![
                json!({"type":"thinking","thinking":"Public reason","signature":"opaque-replay-sentinel"}),
                json!({"type":"text","text":"PublicOpaqueAnswer"}),
            ],
        },
        ProviderProtocol::GoogleGenerateContent => ProviderResponseItem::NativeContent {
            source,
            model: "model".into(),
            prefix_sha256: "0".repeat(64),
            content: vec![
                json!({"text":"PublicOpaqueAnswer","thoughtSignature":"opaque-replay-sentinel"}),
            ],
        },
        ProviderProtocol::ChatCompletions => unreachable!(),
    }
}

#[tokio::test]
async fn privacy_opaque_replay_without_matching_host_policy_blocks_before_socket() {
    let fixture = fixture();
    for protocol in [
        ProviderProtocol::OpenAiResponses,
        ProviderProtocol::AnthropicMessages,
        ProviderProtocol::GoogleGenerateContent,
    ] {
        for receipt in [None, Some("0".repeat(64))] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let store = RequestStore::default();
            store.set_privacy_context(Some(fixture.context.clone()));
            let client = client(
                protocol,
                format!("http://{}/v1", listener.local_addr().unwrap()),
                store,
            );
            let mut source = client.route_identity().unwrap();
            source.privacy_policy_digest = receipt;
            let mut assistant = message("assistant", "PublicOpaqueAnswer");
            assistant.response_items = vec![opaque_item(protocol, source)];
            let result = client
                .chat_stream(
                    &[
                        message("user", "Public initial request"),
                        assistant,
                        message("user", "Public continuation"),
                    ],
                    &[],
                )
                .await;
            assert!(
                matches!(result, Err(ProviderError::PrivacyBlocked { ref detail }) if detail.contains("matching project privacy receipt")),
                "{protocol:?}: {result:?}"
            );
            assert_no_connection(&listener).await;
        }
    }
}

#[test]
fn privacy_matching_opaque_policy_receipts_stay_host_only_and_legacy_serde_stays_compatible() {
    let fixture = fixture();
    for protocol in [
        ProviderProtocol::OpenAiResponses,
        ProviderProtocol::AnthropicMessages,
        ProviderProtocol::GoogleGenerateContent,
    ] {
        let mut source =
            crate::agent::stream::ProviderStateSourceV1::for_test("privacy-test", protocol);
        let legacy = serde_json::to_value(&source).unwrap();
        assert!(legacy.get("privacy_policy_digest").is_none());
        let loaded: crate::agent::stream::ProviderStateSourceV1 =
            serde_json::from_value(legacy).unwrap();
        assert!(loaded.privacy_policy_digest.is_none());
        source.privacy_policy_digest = Some(fixture.context.replay_policy_digest());
        let item = opaque_item(protocol, source);
        assert!(!format!("{item:?}").contains("opaque-replay-sentinel"));
        let mut assistant = message("assistant", "PublicOpaqueAnswer");
        assistant.response_items = vec![item];
        let mut request = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::ToolContinuation,
            "privacy-test",
            "model",
            vec![message("user", "Public initial request"), assistant],
            vec![],
            Default::default(),
        );
        assert!(fixture.context.prepare_request(&mut request).is_ok());
    }
}

#[tokio::test]
async fn privacy_responses_actual_sent_prefix_allows_first_read_and_blocks_new_private_prior_text()
{
    use crate::agent::stream::{consume_stream, ProviderResponseItem};
    for private_authored_before_read in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let path = workspace.join("input.json");
        let source = "{\"client\":\"NewlyPrivateCanary\"}\n";
        std::fs::write(&path, source).unwrap();
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
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let first_server = tokio::spawn(async move {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(4), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let body = read_http_body(&mut socket).await;
            let mut values = Vec::new();
            strings(&body, &mut values);
            let path_alias = values
                .iter()
                .find_map(|value| {
                    let start = value.find("__CA_PRIVATE_")?;
                    let end = start
                        + "__CA_PRIVATE_".len()
                        + value[start + "__CA_PRIVATE_".len()..].find("__")?
                        + 2;
                    Some(value[start..end].to_string())
                })
                .expect("first actual wire request must hide the source path");
            let output = json!([
                {"id":"opaque-first-reasoning","type":"reasoning","summary":[],"encrypted_content":"opaque-actual-prefix-sentinel","status":"completed"},
                {"id":"opaque-first-call","type":"function_call","call_id":"opaque-read","name":"read_file","arguments":json!({"path":path_alias}).to_string(),"status":"completed"}
            ]);
            let event = json!({"type":"response.completed","response":{"status":"completed","output":output}});
            let response = format!("data: {event}\n\n");
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
            (listener, body)
        });
        let store = RequestStore::default();
        store.set_privacy_context(Some(context.clone()));
        let client = client(ProviderProtocol::OpenAiResponses, endpoint, store);
        let first_messages = vec![message(
            "user",
            if private_authored_before_read {
                "Read input.json then discuss NewlyPrivateCanary."
            } else {
                "Read input.json then discuss its structure."
            },
        )];
        let tools = vec![ToolDef {
            def_type: "function".into(),
            function: ToolFunction {
                name: "read_file".into(),
                description: "Public read instruction".into(),
                parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            },
        }];
        let mut stream = client.chat_stream(&first_messages, &tools).await.unwrap();
        let turn = super::scope(Some(context.clone()), consume_stream(&mut stream, |_| {}))
            .await
            .unwrap();
        let (listener, first_wire) = first_server.await.unwrap();
        assert_eq!(
            first_wire.to_string().contains("NewlyPrivateCanary"),
            private_authored_before_read
        );
        let receipt = turn
            .response_items
            .iter()
            .find_map(|item| match item {
                ProviderResponseItem::ResponsesOutput {
                    privacy_prefix_sha256,
                    item,
                    ..
                } if item["type"] == "reasoning" => privacy_prefix_sha256.as_ref(),
                _ => None,
            })
            .expect("actual response decoder must retain its final sent-prefix receipt");
        assert_eq!(
            *receipt,
            crate::provider_replay::responses_prefix(
                &serde_json::to_vec(&first_wire).unwrap(),
                None
            )
            .unwrap()
        );
        let mut assistant = message("assistant", turn.content);
        assistant.tool_calls = Some(
            turn.tool_calls
                .into_iter()
                .map(|call| ToolCallMsg {
                    id: call.id,
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: call.name,
                        arguments: call.arguments,
                        arguments_retrieved_spans: call.arguments_retrieved_spans,
                    },
                })
                .collect(),
        );
        assistant.response_items = turn.response_items;
        let raw = super::render_read(source, 1, 500, false);
        let source_id = context
            .record_read(&path.to_string_lossy(), source, &raw, 1, 500, false)
            .unwrap();
        let mut tool = message("tool", raw.clone());
        tool.tool_call_id = Some("opaque-read".into());
        tool.content = tool
            .content
            .with_retrieved_spans(crate::retrieved_content::full_span(&raw))
            .with_source_receipt(Some(source_id));
        let mut second_messages = first_messages;
        second_messages.push(assistant);
        second_messages.push(tool);
        if private_authored_before_read {
            let result = client.chat_stream(&second_messages, &tools).await;
            assert!(
                matches!(result, Err(ProviderError::PrivacyBlocked { ref detail }) if detail.contains("original prefix")),
                "changed private prefix must be rejected locally: {result:?}"
            );
            assert_no_connection(&listener).await;
        } else {
            let second_server = tokio::spawn(async move {
                let (mut socket, _) =
                    tokio::time::timeout(Duration::from_secs(4), listener.accept())
                        .await
                        .unwrap()
                        .unwrap();
                let body = read_http_body(&mut socket).await;
                socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
                body
            });
            let result = client.chat_stream(&second_messages, &tools).await;
            assert!(
                matches!(result, Err(ProviderError::Http { status: 400, .. })),
                "normal first private read must retain opaque replay utility: {result:?}"
            );
            let second_wire = second_server.await.unwrap();
            assert!(second_wire
                .to_string()
                .contains("opaque-actual-prefix-sentinel"));
            assert!(!second_wire.to_string().contains("NewlyPrivateCanary"));
            assert!(!second_wire.to_string().contains("privacy_prefix_sha256"));
            assert!(!second_wire.to_string().contains("privacy_policy_digest"));
            let preview = context.outbound_snapshot().unwrap();
            assert!(!preview.sent.contains("opaque-actual-prefix-sentinel"));
            assert!(!preview.original.contains("opaque-actual-prefix-sentinel"));
        }
    }
}

#[tokio::test]
async fn privacy_native_actual_sent_prefix_allows_first_read_and_blocks_new_private_prior_text() {
    use crate::agent::stream::{consume_stream, ProviderResponseItem};
    for protocol in [
        ProviderProtocol::AnthropicMessages,
        ProviderProtocol::GoogleGenerateContent,
    ] {
        for private_authored_before_read in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let workspace = temporary.path().join("workspace");
            std::fs::create_dir(&workspace).unwrap();
            let path = workspace.join("input.json");
            let source = "{\"client\":\"NewlyPrivateCanary\"}\n";
            std::fs::write(&path, source).unwrap();
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
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
            let first_server = tokio::spawn(async move {
                let (mut socket, _) =
                    tokio::time::timeout(Duration::from_secs(4), listener.accept())
                        .await
                        .unwrap()
                        .unwrap();
                let body = read_http_body(&mut socket).await;
                let mut values = Vec::new();
                strings(&body, &mut values);
                let path_alias = values
                    .iter()
                    .find_map(|value| {
                        let start = value.find("__CA_PRIVATE_")?;
                        let end = start
                            + "__CA_PRIVATE_".len()
                            + value[start + "__CA_PRIVATE_".len()..].find("__")?
                            + 2;
                        Some(value[start..end].to_string())
                    })
                    .expect("first actual request must hide its source path");
                let frames = match protocol {
                    ProviderProtocol::AnthropicMessages => vec![
                        json!({"type":"message_start","message":{"content":[],"usage":{"input_tokens":1,"output_tokens":0}}}),
                        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
                        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"opaque-native-prefix-sentinel"}}),
                        json!({"type":"content_block_stop","index":0}),
                        json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"opaque-native-read","name":"read_file","input":{}}}),
                        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":json!({"path":path_alias}).to_string()}}),
                        json!({"type":"content_block_stop","index":1}),
                        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":1}}),
                        json!({"type":"message_stop"}),
                    ],
                    ProviderProtocol::GoogleGenerateContent => vec![json!({
                        "candidates":[{"content":{"parts":[{
                            "functionCall":{"name":"read_file","args":{"path":path_alias},"id":"opaque-native-read"},
                            "thoughtSignature":"opaque-native-prefix-sentinel"
                        }]},"finishReason":"STOP"}],
                        "usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1,"totalTokenCount":2}
                    })],
                    _ => unreachable!(),
                };
                let response = frames
                    .into_iter()
                    .map(|frame| format!("data: {frame}\n\n"))
                    .collect::<String>();
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
                (listener, body)
            });
            let store = RequestStore::default();
            store.set_privacy_context(Some(context.clone()));
            let client = client(protocol, endpoint, store);
            let first_messages = vec![message(
                "user",
                if private_authored_before_read {
                    "Read input.json then discuss NewlyPrivateCanary."
                } else {
                    "Read input.json then discuss its structure."
                },
            )];
            let tools = vec![ToolDef {
                def_type: "function".into(),
                function: ToolFunction {
                    name: "read_file".into(),
                    description: "Public read instruction".into(),
                    parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
                },
            }];
            let mut stream = client.chat_stream(&first_messages, &tools).await.unwrap();
            let turn = super::scope(Some(context.clone()), consume_stream(&mut stream, |_| {}))
                .await
                .unwrap();
            let (listener, first_wire) = first_server.await.unwrap();
            assert_eq!(
                first_wire.to_string().contains("NewlyPrivateCanary"),
                private_authored_before_read,
                "{protocol:?} initial authored value remains readable until classified private"
            );
            let receipt = turn
                .response_items
                .iter()
                .find_map(|item| match item {
                    ProviderResponseItem::NativeContent { prefix_sha256, .. } => {
                        Some(prefix_sha256)
                    }
                    _ => None,
                })
                .expect("actual native decoder must retain its final sent-prefix receipt");
            let first_bytes = serde_json::to_vec(&first_wire).unwrap();
            let actual_prefix = match protocol {
                ProviderProtocol::AnthropicMessages => crate::provider_replay::prefix_digest(
                    &crate::provider_replay::prefix_from_body(&first_bytes),
                ),
                ProviderProtocol::GoogleGenerateContent => {
                    crate::google_provider::sent_prefix(&first_bytes).unwrap()
                }
                _ => unreachable!(),
            };
            assert_eq!(*receipt, actual_prefix, "{protocol:?}");
            let tool_id = turn.tool_calls.first().unwrap().id.clone();
            let mut assistant = message("assistant", turn.content);
            assistant.tool_calls = Some(
                turn.tool_calls
                    .into_iter()
                    .map(|call| ToolCallMsg {
                        id: call.id,
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: call.name,
                            arguments: call.arguments,
                            arguments_retrieved_spans: call.arguments_retrieved_spans,
                        },
                    })
                    .collect(),
            );
            assistant.response_items = turn.response_items;
            let raw = super::render_read(source, 1, 500, false);
            let source_id = context
                .record_read(&path.to_string_lossy(), source, &raw, 1, 500, false)
                .unwrap();
            let mut tool = message("tool", raw.clone());
            tool.tool_call_id = Some(tool_id);
            tool.content = tool
                .content
                .with_retrieved_spans(crate::retrieved_content::full_span(&raw))
                .with_source_receipt(Some(source_id));
            let mut second_messages = first_messages;
            second_messages.push(assistant);
            second_messages.push(tool);
            if private_authored_before_read {
                let result = client.chat_stream(&second_messages, &tools).await;
                assert!(
                    matches!(result, Err(ProviderError::PrivacyBlocked { ref detail }) if detail.contains("original prefix")),
                    "{protocol:?} changed private prefix must block locally: {result:?}"
                );
                assert_no_connection(&listener).await;
            } else {
                let second_server = tokio::spawn(async move {
                    let (mut socket, _) =
                        tokio::time::timeout(Duration::from_secs(4), listener.accept())
                            .await
                            .unwrap()
                            .unwrap();
                    let body = read_http_body(&mut socket).await;
                    socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
                    body
                });
                let result = client.chat_stream(&second_messages, &tools).await;
                assert!(
                    matches!(result, Err(ProviderError::Http { status: 400, .. })),
                    "{protocol:?} normal first private read must retain signed replay: {result:?}"
                );
                let second_wire = second_server.await.unwrap();
                assert!(second_wire
                    .to_string()
                    .contains("opaque-native-prefix-sentinel"));
                assert!(!second_wire.to_string().contains("NewlyPrivateCanary"));
                for private_metadata in ["privacy_policy_digest", "prefix_sha256"] {
                    assert!(!second_wire.to_string().contains(private_metadata));
                }
                let preview = context.outbound_snapshot().unwrap();
                assert!(!preview.sent.contains("opaque-native-prefix-sentinel"));
                assert!(!preview.original.contains("opaque-native-prefix-sentinel"));
            }
        }
    }
}
