//! Held-out filename boundary checks through real client and native-tool entry points.
use super::Context;
use crate::agent::client::{
    project_private_request, AgentClient, ChatMessage, FunctionCall, ProviderCallPolicy,
    ToolCallMsg, ToolDef, ToolFunction,
};
use crate::agent::stream::{consume_stream, ProviderError, ProviderResponseItem};
use crate::agent::ProviderProtocol;
use crate::approval::{ApprovalBroker, ApprovalOrigin, BrokerDecision};
use crate::config::{ApprovalRule, PromptCacheMode, ProviderConfig, ProviderTimeoutConfig};
use crate::image_content::ModelContent;
use crate::provider_filter::{FilterHide, ProviderFilterConfig};
use crate::request::{PreparedModelRequest, RequestKind, RequestStore};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

const PRIVATE_DIR: &str = "PrivateCustomerDirectoryCanary";
const PRIVATE_FILE: &str = "PrivateCustomerModuleCanary.rs";
const PUBLIC_SOURCE: &str = "fn public_probe() -> u32 { 1 }\n";
const PROTOCOLS: [ProviderProtocol; 4] = [
    ProviderProtocol::ChatCompletions,
    ProviderProtocol::OpenAiResponses,
    ProviderProtocol::AnthropicMessages,
    ProviderProtocol::GoogleGenerateContent,
];

struct Fixture {
    _root: tempfile::TempDir,
    scope: PathBuf,
    workspace: PathBuf,
    path: PathBuf,
    context: Arc<Context>,
    alias: String,
}

fn config() -> ProviderFilterConfig {
    ProviderFilterConfig {
        enabled: true,
        hide: FilterHide {
            filenames: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn selected_values_config() -> ProviderFilterConfig {
    ProviderFilterConfig {
        structural: true,
        ..config()
    }
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let scope = root.path().join("profile");
    std::fs::create_dir(&scope).unwrap();
    let workspace = root.path().join("workspace");
    let directory = workspace.join(PRIVATE_DIR);
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(PRIVATE_FILE);
    std::fs::write(&path, PUBLIC_SOURCE).unwrap();
    let context = Context::open(scope.clone(), workspace.clone(), Uuid::new_v4(), None).unwrap();
    context.allow_ephemeral();
    assert!(context.filenames_enabled());
    assert!(context.filename_only());
    context.refresh_filenames().unwrap();
    let alias = context
        .mask_known(&format!("{PRIVATE_DIR}/{PRIVATE_FILE}"))
        .unwrap();
    assert!(!alias.contains(PRIVATE_DIR));
    assert!(!alias.contains(PRIVATE_FILE));
    assert!(alias.contains("__CA_PATH_"), "{alias}");
    Fixture {
        _root: root,
        scope,
        workspace,
        path,
        context,
        alias,
    }
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

fn tools() -> Vec<ToolDef> {
    vec![ToolDef {
        def_type: "function".into(),
        function: ToolFunction {
            name: "read_file".into(),
            description: "Read a workspace source using its supplied path.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
        },
    }]
}

fn client(protocol: ProviderProtocol, endpoint: String, context: Arc<Context>) -> AgentClient {
    let store = RequestStore::default();
    store.set_privacy_context(Some(context));
    client_with_store("filename-test", protocol, endpoint, store)
}

fn client_with_store(
    provider_id: &str,
    protocol: ProviderProtocol,
    endpoint: String,
    store: RequestStore,
) -> AgentClient {
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
    AgentClient::with_call_policy(provider_id, &provider, "model", policy, store)
        .with_filter_for_test(Arc::new(
            crate::provider_filter::ProviderFilter::compile(&config()).unwrap(),
        ))
}

async fn read_http_body(socket: &mut TcpStream) -> Value {
    let mut bytes = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        let count = tokio::time::timeout(Duration::from_secs(4), socket.read(&mut chunk))
            .await
            .unwrap()
            .unwrap();
        assert!(count > 0, "closed before complete request");
        bytes.extend_from_slice(&chunk[..count]);
        assert!(bytes.len() <= 1024 * 1024);
        let Some(end) = bytes
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|n| n + 4)
        else {
            continue;
        };
        let length = String::from_utf8_lossy(&bytes[..end])
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        if bytes.len() >= end + length {
            return serde_json::from_slice(&bytes[end..end + length]).unwrap();
        }
    }
}

async fn reject(socket: &mut TcpStream) {
    socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
}

fn assert_private_names_absent(body: &Value) {
    let text = body.to_string();
    for private in [PRIVATE_DIR, PRIVATE_FILE, "PrivateCustomerModuleCanary"] {
        assert!(!text.contains(private), "wire exposed {private}");
    }
    assert!(text.contains("__CA_PATH_"));
    for contract in ["read_file", "path", "PublicTask"] {
        assert!(text.contains(contract), "missing contract {contract}");
    }
    for internal in [
        "manifest_sha256",
        "registry_id",
        "filename_snapshot",
        "privacy_revision",
    ] {
        assert!(
            !text.contains(internal),
            "sent local registry metadata {internal}"
        );
    }
}

#[test]
fn filename_reasoning_effort_controls_are_exact_enums_only_at_protocol_locations() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        let canonical = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];
        let private_controls = fixture.workspace.join("private_controls");
        std::fs::create_dir(&private_controls).unwrap();
        for value in canonical {
            std::fs::write(private_controls.join(value), "private filename").unwrap();
        }
        let unknown = ["ultra", "x_high", "HIGH", " high "];
        for value in unknown {
            std::fs::write(fixture.workspace.join(value), "private filename").unwrap();
        }
        fixture.context.refresh_filenames().unwrap();
        let revision = fixture.context.policy().revision;
        let fingerprint = fixture.context.policy_fingerprint();
        for value in canonical {
            for body in [
                json!({"model":"model","reasoning_effort":value,"messages":[]}),
                json!({"model":"model","reasoning":{"effort":value},"input":[]}),
            ] {
                fixture
                    .context
                    .validate_wire(&serde_json::to_vec(&body).unwrap(), revision, &fingerprint)
                    .unwrap();
            }
            for body in [
                json!({"data":{"reasoning_effort":value}}),
                json!({"data":{"reasoning":{"effort":value}}}),
                json!({"messages":[{"role":"user","content":value}]}),
                json!({"input":[{"type":"function_call","name":"read_file","call_id":"public-call","arguments":json!({"path":value}).to_string()}]}),
            ] {
                assert!(fixture
                    .context
                    .validate_wire(&serde_json::to_vec(&body).unwrap(), revision, &fingerprint)
                    .is_err());
            }
        }
        for value in unknown {
            for body in [
                json!({"reasoning_effort":value}),
                json!({"reasoning":{"effort":value}}),
            ] {
                assert!(fixture
                    .context
                    .validate_wire(&serde_json::to_vec(&body).unwrap(), revision, &fingerprint)
                    .is_err());
            }
        }
    });
}

#[test]
fn filename_host_guidance_is_exact_protocol_control_and_keeps_dynamic_prefix_private() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        std::fs::write(fixture.workspace.join("discovered"), "private filename").unwrap();
        std::fs::write(
            fixture.workspace.join("PrivateDescriptionPrefix317.txt"),
            "private filename",
        )
        .unwrap();
        fixture.context.refresh_filenames().unwrap();
        let mut definitions = tools();
        definitions[0].function.description = "Read PrivateDescriptionPrefix317.txt safely.".into();
        let mut request = PreparedModelRequest::new(
            RequestKind::MainTurn,
            "filename-test",
            "model",
            vec![message("user", "PublicTask")],
            definitions,
            Default::default(),
        );
        let revision = fixture.context.prepare_request(&mut request).unwrap();
        let fingerprint = fixture.context.policy_fingerprint();
        let description = request.tools[0].function.description.clone();
        let schema = request.tools[0].function.parameters.clone();
        assert!(!description.contains("PrivateDescriptionPrefix317.txt"));
        assert!(description.contains("complete discovered filename references"));
        assert_ne!(
            fixture.context.mask_known("discovered").unwrap(),
            "discovered"
        );
        for body in [
            json!({"tools":[{"type":"function","function":{"name":"read_file","description":description,"parameters":schema}}]}),
            json!({"tools":[{"type":"function","name":"read_file","description":description,"parameters":schema}]}),
            json!({"tools":[{"name":"read_file","description":description,"input_schema":schema}]}),
            json!({"tools":[{"functionDeclarations":[{"name":"read_file","description":description,"parameters":schema}]}]}),
        ] {
            fixture
                .context
                .validate_wire(&serde_json::to_vec(&body).unwrap(), revision, &fingerprint)
                .unwrap();
        }
        let suffix = &description[description
            .find(" Use complete discovered filename references")
            .unwrap()..];
        for body in [
            json!({"messages":[{"role":"user","content":description}]}),
            json!({"data":{"description":description}}),
            json!({"input":[{"type":"function_call","name":"read_file","call_id":"public-call","arguments":json!({"path":fixture.alias}).to_string(),"description":description}]}),
            json!({"tools":[{"type":"function","function":{"name":"read_file","description":format!("PrivateDescriptionPrefix317.txt{suffix}"),"parameters":schema}}]}),
            json!({"tools":[{"type":"function","function":{"name":"read_file","description":description.replace("complete discovered", "changed discovered"),"parameters":schema}}]}),
            json!({"tools":[{"type":"function","function":{"name":"custom_reader","description":description,"parameters":schema}}]}),
        ] {
            assert!(fixture
                .context
                .validate_wire(&serde_json::to_vec(&body).unwrap(), revision, &fingerprint)
                .is_err());
        }
    });
}

#[test]
fn hrs317_system_evidence_guidance_keeps_public_controls_only_in_system_envelopes() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        std::fs::write(fixture.workspace.join("1"), "private filename").unwrap();
        fixture.context.refresh_filenames().unwrap();
        let guide = crate::context_pruning::EVIDENCE_GUIDANCE;
        assert_ne!(fixture.context.mask_known(guide).unwrap(), guide);
        let revision = fixture.context.policy().revision;
        let fingerprint = fixture.context.policy_fingerprint();
        for body in [
            json!({"messages":[{"role":"system","content":guide}]}),
            json!({"input":[{"role":"system","content":[{"type":"input_text","text":guide}]}]}),
            json!({"system":[{"type":"text","text":guide}]}),
            json!({"systemInstruction":{"parts":[{"text":guide}]}}),
        ] {
            fixture
                .context
                .validate_wire(&serde_json::to_vec(&body).unwrap(), revision, &fingerprint)
                .unwrap();
        }
        for body in [
            json!({"messages":[{"role":"user","content":guide}]}),
            json!({"input":[{"role":"user","content":[{"type":"input_text","text":guide}]}]}),
            json!({"messages":[{"role":"system","content":format!("{guide} 1")}]}),
            json!({"system":[{"type":"text","text":format!("1 {guide}")}]}),
            json!({"systemInstruction":{"parts":[{"text":format!("{guide} 1")}]}}),
        ] {
            assert!(fixture
                .context
                .validate_wire(&serde_json::to_vec(&body).unwrap(), revision, &fingerprint)
                .is_err());
        }
        let mut request = PreparedModelRequest::new(
            RequestKind::Headless,
            "filename-test",
            "model",
            vec![message("system", guide), message("user", guide)],
            tools(),
            Default::default(),
        );
        fixture.context.prepare_request(&mut request).unwrap();
        assert_eq!(request.messages[0].content.as_str(), Some(guide));
        assert_ne!(request.messages[1].content.as_str(), Some(guide));
    });
}

#[test]
fn hrs317_artifact_listing_projects_only_native_rows_before_authored_notes() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        let view = fixture
            .context
            .prepare_command_workspace(
                "true",
                fixture.workspace.to_str().unwrap(),
                std::slice::from_ref(&fixture.workspace),
            )
            .unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(&fixture.workspace).unwrap();
        let arguments = json!({"path":view.artifact_directory});
        let native_arguments = json!({"path":view.cwd});
        let named = "[CA evidence: real file]";
        std::fs::write(std::path::Path::new(&view.cwd).join(named), "bytes").unwrap();
        let raw = format!("{PRIVATE_DIR}/\n{named}");
        crate::context_observation::capture_native_execution_scoped(
            "list_dir",
            &native_arguments,
            &raw,
            true,
            &workspace,
            None,
            Some(&fixture.context),
        )
        .unwrap();
        let observation = crate::context_observation::bind_native_result(
            "list_dir",
            &arguments.to_string(),
            &raw,
        )
        .unwrap();
        let mut assistant = message("assistant", "List the command artifact");
        assistant.tool_calls = Some(vec![ToolCallMsg {
            id: "artifact-list".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "list_dir".into(),
                arguments: arguments.to_string(),
                arguments_retrieved_spans: vec![],
            },
        }]);
        let mut tool = message("tool", &raw);
        tool.tool_call_id = Some("artifact-list".into());
        tool.content = tool.content.with_context_observation(Some(observation));
        let mut request = PreparedModelRequest::new(
            RequestKind::ToolContinuation,
            "filename-test",
            "model",
            vec![assistant, tool],
            vec![],
            Default::default(),
        );
        assert!(request.messages[1]
            .content
            .as_str()
            .unwrap()
            .contains("command-copy artifact"));
        fixture.context.prepare_request(&mut request).unwrap();
        let projected = request.messages[1].content.as_str().unwrap();
        assert!(projected.starts_with("__CA_PATH_"));
        assert!(!projected.contains(PRIVATE_DIR));
        assert!(projected.contains("command-copy artifact"));
        assert!(fixture
            .context
            .resolve_filename_path("__CA_PATH_unknown__", false)
            .is_err());
        // A real filename resembling a note is projected as a second native row.
        assert!(projected
            .lines()
            .take(2)
            .all(|row| row.starts_with("__CA_PATH_")));
        assert!(!projected.contains(named));
        let second = projected.lines().nth(1).unwrap();
        assert_eq!(
            fixture
                .context
                .resolve_filename_path(second, false)
                .unwrap(),
            std::path::Path::new(&view.cwd).join(named)
        );
    });
}

#[test]
fn selected_values_empty_file_listing_hides_the_filename_before_send() {
    crate::provider_filter::with_test_filter(&selected_values_config(), || {
        let root = tempfile::tempdir().unwrap();
        let scope = root.path().join("profile");
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&scope).unwrap();
        std::fs::create_dir(&workspace).unwrap();
        let secret = "porn.rtf";
        let hidden = ".DS_Store";
        std::fs::write(workspace.join(secret), "{\\rtf1\\ansi}\n").unwrap();
        std::fs::write(workspace.join(hidden), [0_u8, 0, 0, 1]).unwrap();
        let context = Context::open(scope, workspace.clone(), Uuid::new_v4(), None).unwrap();
        context.allow_ephemeral();
        assert!(context.filenames_enabled());
        assert!(
            !context.filename_only(),
            "selected values must not fall back to filename-only content rules"
        );
        assert_eq!(
            context.summary(),
            "Project privacy · Selected values only · filenames on · 0 selected values"
        );
        let listing = format!("{hidden}\n{secret}");
        let mut assistant = message("assistant", "List this directory");
        assistant.tool_calls = Some(vec![ToolCallMsg {
            id: "list-empty".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "list_dir".into(),
                arguments: r#"{"path":"."}"#.into(),
                arguments_retrieved_spans: vec![],
            },
        }]);
        let mut listed = message("tool", &listing);
        listed.tool_call_id = Some("list-empty".into());
        listed.content = listed
            .content
            .with_retrieved_spans(crate::retrieved_content::full_span(&listing))
            .with_tool_result_status(true);
        let absolute = workspace.join(secret);
        let globbed = absolute.to_string_lossy().into_owned();
        let mut glob_call = message("assistant", "Find files");
        glob_call.tool_calls = Some(vec![ToolCallMsg {
            id: "glob-empty".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "glob".into(),
                arguments: r#"{"path":".","pattern":"*"}"#.into(),
                arguments_retrieved_spans: vec![],
            },
        }]);
        let mut glob_result = message("tool", &globbed);
        glob_result.tool_call_id = Some("glob-empty".into());
        glob_result.content = glob_result
            .content
            .with_retrieved_spans(crate::retrieved_content::full_span(&globbed))
            .with_tool_result_status(true);
        let mut request = PreparedModelRequest::new(
            RequestKind::ToolContinuation,
            "filename-test",
            "model",
            vec![assistant, listed, glob_call, glob_result],
            vec![],
            Default::default(),
        );
        context.prepare_request(&mut request).unwrap();
        let provider = serde_json::to_string(&request.messages).unwrap();
        assert!(
            !provider.contains(secret),
            "provider copy still contains the empty file name: {provider}"
        );
        assert!(
            !provider.contains("DS_Store"),
            "provider copy still contains the dotfile name: {provider}"
        );
        assert!(provider.contains("__CA_PATH_"), "{provider}");
        let listed_rows = request.messages[1].content.as_str().unwrap();
        assert!(listed_rows.lines().all(|row| row.starts_with("__CA_PATH_")));
        assert_eq!(
            context
                .resolve_filename_path(listed_rows.lines().nth(1).unwrap(), false)
                .unwrap(),
            absolute
        );
        let echoed = listed_rows.to_owned();
        let restored = context.restore_text(&echoed).unwrap();
        assert!(restored.contains(secret), "{restored}");
        assert!(restored.contains(hidden), "{restored}");
        let projected = context
            .project_tool_output("list_dir", r#"{"path":"."}"#, &listing)
            .unwrap();
        assert!(!projected.contains(secret), "{projected}");
        assert!(!projected.contains("DS_Store"), "{projected}");
        let again = context
            .project_tool_output("list_dir", r#"{"path":"."}"#, &projected)
            .unwrap();
        assert_eq!(again, projected);
        let (restored_rows, spans) = context.restore_with_spans(&projected).unwrap();
        assert!(restored_rows.contains(secret));
        assert_eq!(spans.len(), 2, "{spans:?} {restored_rows}");
        assert!(spans
            .iter()
            .all(|span| span.end - span.start < restored_rows.len()));
        let leaf = projected
            .lines()
            .next()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap();
        let (leaf_text, leaf_spans) = context.restore_with_spans(leaf).unwrap();
        assert_eq!(leaf_text, leaf);
        assert!(leaf_spans.is_empty(), "{leaf_spans:?}");
    });
}

#[test]
fn list_dir_projection_keeps_real_rows_when_one_row_is_missing() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        std::fs::write(fixture.workspace.join("keep.txt"), "ok\n").unwrap();
        let listing = "keep.txt\nnot-on-disk.txt";
        let projected = fixture
            .context
            .project_tool_output("list_dir", r#"{"path":"."}"#, listing)
            .expect("one missing row must not withhold the listing");
        assert!(!projected.contains("keep.txt"), "{projected}");
        assert!(!projected.contains("not-on-disk"), "{projected}");
        assert!(projected.contains("__CA_PATH_"), "{projected}");
        assert!(projected.contains("withheld"), "{projected}");
        assert_ne!(projected.trim(), "[listing withheld]");
    });
}

#[test]
fn selected_values_preview_samples_an_empty_file_without_sending_its_name() {
    crate::provider_filter::with_test_filter(&selected_values_config(), || {
        let root = tempfile::tempdir().unwrap();
        let scope = root.path().join("profile");
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&scope).unwrap();
        std::fs::create_dir(&workspace).unwrap();
        let secret = "empty-canary.rtf";
        let hidden = ".HiddenCanary";
        std::fs::write(workspace.join(secret), "").unwrap();
        std::fs::write(workspace.join(hidden), [1_u8]).unwrap();
        let context = Context::open(scope, workspace, Uuid::new_v4(), None).unwrap();
        context.allow_ephemeral();
        let task = context
            .with_task_filter(crate::task_filter::Policy::default())
            .unwrap();
        let preview = task.preview_task("list this dir").unwrap();
        assert!(
            !preview.provider_copy.contains(secret),
            "{}",
            preview.provider_copy
        );
        assert!(
            !preview.provider_copy.contains("HiddenCanary"),
            "{}",
            preview.provider_copy
        );
        let names = preview
            .filename_samples
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&secret), "{:?}", preview.filename_samples);
        assert!(names.contains(&hidden), "{:?}", preview.filename_samples);
        assert!(preview
            .filename_samples
            .iter()
            .all(|(_, alias)| alias.contains("__CA_PATH_")));
    });
}

#[test]
fn filename_final_transport_all_four_protocols_hides_continuation_history() {
    crate::provider_filter::with_test_filter(&config(), || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let fixture = fixture();
                for protocol in PROTOCOLS {
                    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
                    let server = tokio::spawn(async move {
                        let mut bodies = Vec::new();
                        for _ in 0..2 {
                            let (mut socket, _) =
                                tokio::time::timeout(Duration::from_secs(6), listener.accept())
                                    .await
                                    .unwrap()
                                    .unwrap();
                            bodies.push(read_http_body(&mut socket).await);
                            reject(&mut socket).await;
                        }
                        bodies
                    });
                    let client = client(protocol, endpoint, fixture.context.clone());
                    let mut messages = vec![message(
                        "user",
                        format!("PublicTask: read {PRIVATE_DIR}/{PRIVATE_FILE}"),
                    )];
                    for turn in 0..2 {
                        if turn == 1 {
                            let mut assistant = message("assistant", "Public read request");
                            assistant.tool_calls = Some(vec![ToolCallMsg {
                                id: "filename-read".into(),
                                call_type: "function".into(),
                                function: FunctionCall {
                                    name: "read_file".into(),
                                    arguments: json!({"path":fixture.path}).to_string(),
                                    arguments_retrieved_spans: Vec::new(),
                                },
                            }]);
                            messages.push(assistant);
                            let mut result = message(
                                "tool",
                                format!("{PRIVATE_DIR}/{PRIVATE_FILE}:\n{PUBLIC_SOURCE}"),
                            );
                            result.tool_call_id = Some("filename-read".into());
                            messages.push(result);
                            messages.push(message("user", "PublicSecondTurn: compare again."));
                        }
                        let original = serde_json::to_vec(&messages).unwrap();
                        let result = client.chat_stream(&messages, &tools()).await;
                        assert!(
                            matches!(result, Err(ProviderError::Http { status: 400, .. })),
                            "{protocol:?}: {result:?}"
                        );
                        assert_eq!(
                            serde_json::to_vec(&messages).unwrap(),
                            original,
                            "local history changed"
                        );
                    }
                    let bodies = server.await.unwrap();
                    for body in &bodies {
                        assert_private_names_absent(body);
                    }
                    assert!(bodies[1].to_string().contains("PublicSecondTurn"));
                    assert!(
                        bodies[1].to_string().contains("public_probe"),
                        "filename mode hid public source content"
                    );
                    assert!(
                        bodies[1].to_string().contains(&fixture.alias),
                        "alias changed between transmissions"
                    );
                }
                assert_eq!(
                    std::fs::read_to_string(&fixture.path).unwrap(),
                    PUBLIC_SOURCE
                );
            });
    });
}

#[test]
fn filename_model_call_broker_real_path_approval_and_native_edit_roundtrip() {
    crate::provider_filter::with_test_filter(&config(), || {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let fixture = fixture();
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
            let alias = fixture.alias.clone();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let body = read_http_body(&mut socket).await;
                let arguments = json!({"path":alias}).to_string();
                let split = arguments.len() / 2;
                let first = json!({"id":"filename-turn","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"filename-model-read","type":"function","function":{"name":"read_file","arguments":&arguments[..split]}}]},"finish_reason":null}]});
                let last = json!({"id":"filename-turn","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":&arguments[split..]}}]},"finish_reason":"tool_calls"}]});
                let events = format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n");
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{events}", events.len()).as_bytes()).await.unwrap();
                body
            });
            let client = client(ProviderProtocol::ChatCompletions, endpoint, fixture.context.clone());
            let messages = vec![message("user", format!("PublicTask: read {PRIVATE_DIR}/{PRIVATE_FILE}"))];
            let mut stream = client.chat_stream(&messages, &tools()).await.unwrap();
            let turn = super::scope(Some(fixture.context.clone()), consume_stream(&mut stream, |_|{})).await.unwrap();
            assert_private_names_absent(&server.await.unwrap());
            assert_eq!(turn.tool_calls.len(), 1);
            let call = &turn.tool_calls[0];
            assert_eq!(call.name, "read_file");
            let model_arguments: Value = serde_json::from_str(&call.arguments).unwrap();
            assert_eq!(model_arguments["path"], fixture.alias);
            let workspace = crate::workspace::WorkspaceContext::discover(fixture.workspace.to_str().unwrap()).unwrap();
            super::scope(Some(fixture.context.clone()), async {
                let rules = [ApprovalRule::AllowTool("read_file".into()), ApprovalRule::AllowTool("write_file".into())];
                let broker = ApprovalBroker::new(&workspace, &rules, ApprovalOrigin::Main);
                let (read, decision) = broker.prepare_formatted("read_file", &model_arguments, crate::agent::registry::ToolOrigin::BuiltIn, false, crate::sandbox::SandboxMode::Off).await.unwrap();
                assert_eq!(decision, BrokerDecision::AllowedByRule);
                assert_eq!(read.arguments["path"], fixture.path.to_string_lossy().as_ref());
                assert_eq!(model_arguments["path"], fixture.alias, "resolution mutated model call");
                let result = crate::agent::tools::execute_prepared_tool_in_workspace_policy_owned("read_file", &read.arguments, &workspace, 5, crate::sandbox::SandboxMode::Off, false, crate::jobs::JobOwner::new(Some(fixture.context.session_id))).await;
                assert!(result.is_success(), "{}", result.model_text());
                assert!(result.model_text().contains("public_probe"));
                let edited = "fn public_probe() -> u32 { 2 }\n";
                let args = json!({"path":fixture.alias,"content":edited});
                let deny = [ApprovalRule::DenyPath {tool:"write_file".into(),path_pattern:fixture.path.to_string_lossy().into_owned()}];
                let denied = ApprovalBroker::new(&workspace, &deny, ApprovalOrigin::Main);
                let (_, decision) = denied.prepare_formatted("write_file", &args, crate::agent::registry::ToolOrigin::BuiltIn, false, crate::sandbox::SandboxMode::Off).await.unwrap();
                assert_eq!(decision, BrokerDecision::DeniedByRule, "approval failed to inspect real path");
                assert_eq!(std::fs::read_to_string(&fixture.path).unwrap(), PUBLIC_SOURCE);
                let (write, _) = broker.prepare_formatted("write_file", &args, crate::agent::registry::ToolOrigin::BuiltIn, false, crate::sandbox::SandboxMode::Off).await.unwrap();
                let result = crate::agent::tools::execute_prepared_tool_in_workspace_policy_owned("write_file", &write.arguments, &workspace, 5, crate::sandbox::SandboxMode::Off, false, crate::jobs::JobOwner::new(Some(fixture.context.session_id))).await;
                assert!(result.is_success(), "{}", result.model_text());
                assert_eq!(std::fs::read_to_string(&fixture.path).unwrap(), edited);
                assert!(broker.prepare_formatted("run_command", &json!({"command":"true"}), crate::agent::registry::ToolOrigin::BuiltIn, false, crate::sandbox::SandboxMode::Off).await.is_ok());
                assert!(broker.prepare_formatted("read_file", &json!({"path":format!("{}/../outside", fixture.alias)}), crate::agent::registry::ToolOrigin::BuiltIn, false, crate::sandbox::SandboxMode::Off).await.is_err());
            }).await;
        });
    });
}

#[test]
fn filename_registry_saved_reference_restores_same_path_after_restart() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        let session = fixture.context.session_id;
        let mut request = PreparedModelRequest::new(
            RequestKind::Headless,
            "filename-test",
            "model",
            vec![message(
                "user",
                format!("Read {PRIVATE_DIR}/{PRIVATE_FILE}"),
            )],
            tools(),
            Default::default(),
        );
        fixture.context.prepare_request(&mut request).unwrap();
        let mut reference = fixture
            .context
            .state
            .lock()
            .unwrap()
            .registry
            .stage(&fixture.scope)
            .unwrap();
        reference.optional_policy = true;
        let alias = fixture.alias.clone();
        let scope = fixture.scope.clone();
        let workspace = fixture.workspace.clone();
        let path = fixture.path.clone();
        drop(fixture.context);
        let resumed = Context::open(scope, workspace, session, Some(reference)).unwrap();
        resumed.allow_ephemeral();
        resumed.refresh_filenames().unwrap();
        assert_eq!(
            resumed
                .mask_known(&format!("{PRIVATE_DIR}/{PRIVATE_FILE}"))
                .unwrap(),
            alias
        );
        let args = resumed
            .prepare_edit_ticket("read_file", &json!({"path":alias}))
            .unwrap();
        assert_eq!(args["path"], path.to_string_lossy().as_ref());
        assert!(
            !resumed.policy_path.exists(),
            "filename mode created a project-content policy"
        );
    });
}

#[test]
fn filename_directory_listings_and_scoped_search_keep_executable_full_aliases() {
    crate::provider_filter::with_test_filter(&config(), || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let fixture = fixture();
                let other_directory = "SecondPrivateDirectoryCanary";
                std::fs::create_dir(fixture.workspace.join(other_directory)).unwrap();
                let other_file = fixture.workspace.join(other_directory).join(PRIVATE_FILE);
                std::fs::write(&other_file, PUBLIC_SOURCE).unwrap();
                fixture.context.refresh_filenames().unwrap();
                let workspace = crate::workspace::WorkspaceContext::discover(
                    fixture.workspace.to_str().unwrap(),
                )
                .unwrap();
                super::scope(Some(fixture.context.clone()), async {
                    let rules = [
                        ApprovalRule::AllowTool("list_dir".into()),
                        ApprovalRule::AllowTool("search_files".into()),
                        ApprovalRule::AllowTool("glob".into()),
                    ];
                    let broker = ApprovalBroker::new(&workspace, &rules, ApprovalOrigin::Main);
                    let mut messages =
                        vec![message("user", "PublicTask: list supplied directories")];
                    for (index, directory) in [PRIVATE_DIR, other_directory].iter().enumerate() {
                        let directory_alias = fixture.context.mask_known(directory).unwrap();
                        let args = json!({"path":directory_alias});
                        let (prepared, decision) = broker
                            .prepare_formatted(
                                "list_dir",
                                &args,
                                crate::agent::registry::ToolOrigin::BuiltIn,
                                false,
                                crate::sandbox::SandboxMode::Off,
                            )
                            .await
                            .unwrap();
                        assert_eq!(decision, BrokerDecision::AllowedByRule);
                        let result =
                            crate::agent::tools::execute_prepared_tool_in_workspace_policy_owned(
                                "list_dir",
                                &prepared.arguments,
                                &workspace,
                                5,
                                crate::sandbox::SandboxMode::Off,
                                false,
                                crate::jobs::JobOwner::new(Some(fixture.context.session_id)),
                            )
                            .await;
                        assert!(result.is_success(), "{}", result.model_text());
                        assert_eq!(
                            result.model_text().trim(),
                            PRIVATE_FILE,
                            "local listing was aliased"
                        );
                        let call_id = format!("listing-{index}");
                        let mut assistant = message("assistant", "List this directory");
                        assistant.tool_calls = Some(vec![ToolCallMsg {
                            id: call_id.clone(),
                            call_type: "function".into(),
                            function: FunctionCall {
                                name: "list_dir".into(),
                                arguments: args.to_string(),
                                arguments_retrieved_spans: Vec::new(),
                            },
                        }]);
                        let mut output = message("tool", result.model_text());
                        output.tool_call_id = Some(call_id);
                        messages.push(assistant);
                        messages.push(output);
                    }
                    let before = serde_json::to_vec(&messages).unwrap();
                    let mut request = PreparedModelRequest::new(
                        RequestKind::ToolContinuation,
                        "filename-test",
                        "model",
                        messages.clone(),
                        vec![],
                        Default::default(),
                    );
                    fixture.context.prepare_request(&mut request).unwrap();
                    let mut full_aliases = Vec::new();
                    for (index, expected_path) in
                        [fixture.path.clone(), other_file].iter().enumerate()
                    {
                        let id = format!("listing-{index}");
                        let text = request
                            .messages
                            .iter()
                            .find(|message| message.tool_call_id.as_deref() == Some(id.as_str()))
                            .unwrap()
                            .content
                            .as_str()
                            .unwrap()
                            .trim();
                        assert!(
                            text.starts_with("__CA_PATH_"),
                            "listing did not yield executable path: {text}"
                        );
                        assert!(!text.contains(PRIVATE_FILE));
                        let resolved = fixture
                            .context
                            .prepare_edit_ticket("read_file", &json!({"path":text}))
                            .unwrap();
                        assert_eq!(resolved["path"], expected_path.to_string_lossy().as_ref());
                        full_aliases.push(text.to_string());
                    }
                    assert_ne!(
                        full_aliases[0], full_aliases[1],
                        "duplicate basenames lost directory identity"
                    );
                    assert_eq!(
                        serde_json::to_vec(&messages).unwrap(),
                        before,
                        "projection changed local listings"
                    );
                    let directory_alias = fixture.context.mask_known(PRIVATE_DIR).unwrap();
                    for (name, pattern) in [("search_files", "public_probe"), ("glob", "*.rs")] {
                        let (prepared, _) = broker
                            .prepare_formatted(
                                name,
                                &json!({"path":directory_alias,"pattern":pattern}),
                                crate::agent::registry::ToolOrigin::BuiltIn,
                                false,
                                crate::sandbox::SandboxMode::Off,
                            )
                            .await
                            .unwrap();
                        let result =
                            crate::agent::tools::execute_prepared_tool_in_workspace_policy_owned(
                                name,
                                &prepared.arguments,
                                &workspace,
                                5,
                                crate::sandbox::SandboxMode::Off,
                                false,
                                crate::jobs::JobOwner::new(Some(fixture.context.session_id)),
                            )
                            .await;
                        assert!(result.is_success(), "{name}: {}", result.model_text());
                        assert!(
                            result.model_text().contains(PRIVATE_FILE),
                            "local {name} lost original filename"
                        );
                        let sent = fixture.context.mask_known(result.model_text()).unwrap();
                        assert!(!sent.contains(PRIVATE_FILE));
                        assert!(!sent.contains(PRIVATE_DIR));
                        assert!(sent.contains(&fixture.alias));
                    }
                    assert!(fixture
                        .context
                        .prepare_edit_ticket(
                            "glob",
                            &json!({"path":".","pattern":format!("{directory_alias}/*.rs")})
                        )
                        .is_ok());
                    assert!(fixture
                        .context
                        .prepare_edit_ticket(
                            "search_files",
                            &json!({"path":".","pattern":fixture.alias})
                        )
                        .is_ok());
                })
                .await;
            });
    });
}

#[test]
fn filename_shared_child_and_fork_keep_old_aliases_and_separate_new_allocations() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        let shared = super::for_session(fixture.context.session_id).unwrap();
        assert!(Arc::ptr_eq(&shared, &fixture.context));
        assert_eq!(
            shared
                .mask_known(&format!("{PRIVATE_DIR}/{PRIVATE_FILE}"))
                .unwrap(),
            fixture.alias
        );
        let fork = fixture.context.fork_ephemeral(Uuid::new_v4()).unwrap();
        assert_eq!(
            fork.mask_known(&format!("{PRIVATE_DIR}/{PRIVATE_FILE}"))
                .unwrap(),
            fixture.alias
        );
        let new_name = "NewPrivateAfterForkCanary.rs";
        std::fs::write(
            fixture.workspace.join(new_name),
            "fn new_public_probe() {}\n",
        )
        .unwrap();
        fixture.context.refresh_filenames().unwrap();
        fork.refresh_filenames().unwrap();
        let parent_alias = fixture.context.mask_known(new_name).unwrap();
        let fork_alias = fork.mask_known(new_name).unwrap();
        assert_ne!(
            parent_alias, fork_alias,
            "fork future allocation reused the parent's alias"
        );
        for (context, alias) in [(&fixture.context, parent_alias), (&fork, fork_alias)] {
            let restored = context
                .prepare_edit_ticket("read_file", &json!({"path":alias}))
                .unwrap();
            assert_eq!(
                restored["path"],
                fixture.workspace.join(new_name).to_string_lossy().as_ref()
            );
            assert_eq!(
                context
                    .mask_known(&format!("{PRIVATE_DIR}/{PRIVATE_FILE}"))
                    .unwrap(),
                fixture.alias
            );
        }
    });
}

#[test]
fn filename_mode_does_not_enable_disabled_saved_rules_and_honors_later_enable() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        let disabled = super::policy::Policy {
            enabled: false,
            rules: vec![super::policy::Rule {
                file: "*.rs".into(),
                selector: "**".into(),
                action: super::policy::Action::Hide,
            }],
            ..Default::default()
        };
        fixture.context.save_policy(disabled).unwrap();
        assert!(fixture.context.filename_only());
        assert!(
            fixture.context.policy().rules.is_empty(),
            "filename mode activated disabled source selections"
        );
        assert_eq!(
            fixture
                .context
                .project_source(&format!("{PRIVATE_DIR}/{PRIVATE_FILE}"), PUBLIC_SOURCE)
                .unwrap()
                .text,
            PUBLIC_SOURCE
        );
        let mut enabled = fixture.context.stored_policy();
        enabled.enabled = true;
        fixture.context.save_policy(enabled).unwrap();
        assert!(!fixture.context.filename_only());
        let result = fixture.context.prepare_edit_ticket(
            "write_file",
            &json!({"path":fixture.alias,"content":"fn changed() {}\n"}),
        );
        assert!(
            result.is_err(),
            "enabling source privacy bypassed read-before-edit"
        );
        assert_eq!(
            std::fs::read_to_string(&fixture.path).unwrap(),
            PUBLIC_SOURCE
        );
    });
}

#[test]
fn chatgpt_shim_wire_keeps_filenames_and_omits_tools() {
    crate::provider_filter::with_test_filter(&config(), || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let fixture = fixture();
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
                let server = tokio::spawn(async move {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let body = read_http_body(&mut socket).await;
                    reject(&mut socket).await;
                    body
                });
                let store = RequestStore::default();
                store.set_privacy_context(Some(fixture.context.clone()));
                let client = client_with_store(
                    "chatgpt-shim",
                    ProviderProtocol::ChatCompletions,
                    endpoint,
                    store,
                );
                let mut assistant = message("assistant", "Looking at the file");
                assistant.tool_calls = Some(vec![ToolCallMsg {
                    id: "filename-read".into(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: "read_file".into(),
                        arguments: json!({"path": fixture.path}).to_string(),
                        arguments_retrieved_spans: Vec::new(),
                    },
                }]);
                let mut result = message("tool", format!("{PRIVATE_DIR}/{PRIVATE_FILE} listed"));
                result.tool_call_id = Some("filename-read".into());
                let messages = vec![
                    message(
                        "user",
                        format!("Plan workflow goal: keep {PRIVATE_DIR}/{PRIVATE_FILE}"),
                    ),
                    assistant,
                    result,
                ];
                let result = client.chat_stream(&messages, &tools()).await;
                assert!(
                    matches!(result, Err(ProviderError::Http { status: 400, .. })),
                    "{result:?}"
                );
                let body = server.await.unwrap();
                let text = body.to_string();
                assert!(text.contains(PRIVATE_DIR), "{text}");
                assert!(text.contains(PRIVATE_FILE), "{text}");
                assert!(text.contains("Plan workflow goal"), "{text}");
                assert!(text.contains("Looking at the file"), "{text}");
                assert!(!text.contains("__CA_NAME_"), "{text}");
                assert!(!text.contains("__CA_PATH_"), "{text}");
                assert!(!text.contains("read_file"), "{text}");
                assert!(!text.contains("tool_calls"), "{text}");
                assert!(body.get("tools").is_none(), "{body}");
            });
    });
}

#[test]
fn filename_private_schema_keys_and_missing_context_block_before_socket() {
    crate::provider_filter::with_test_filter(&config(), || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let fixture = fixture();
                for missing_context in [false, true] {
                    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
                    let store = RequestStore::default();
                    let mut definitions = tools();
                    if !missing_context {
                        store.set_privacy_context(Some(fixture.context.clone()));
                        definitions[0].function.parameters["properties"][PRIVATE_FILE] =
                            json!({"type":"string"});
                    }
                    let canonical = serde_json::to_vec(&definitions).unwrap();
                    let client = client_with_store(
                        "filename-test",
                        ProviderProtocol::ChatCompletions,
                        endpoint,
                        store,
                    );
                    let result = client
                        .chat_stream(&[message("user", "PublicTask")], &definitions)
                        .await;
                    assert!(
                        matches!(result, Err(ProviderError::PrivacyBlocked { .. })),
                        "missing_context={missing_context}: {result:?}"
                    );
                    assert_eq!(
                        serde_json::to_vec(&definitions).unwrap(),
                        canonical,
                        "schema was mutated"
                    );
                    assert!(
                        tokio::time::timeout(Duration::from_millis(150), listener.accept())
                            .await
                            .is_err(),
                        "blocked request opened a provider connection"
                    );
                }
            });
    });
}

#[test]
fn filename_alias_shaped_literal_listing_still_returns_executable_paths() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        let literal = "__CA_NAME_fixture__";
        for directory in ["left", "right"] {
            std::fs::create_dir(fixture.workspace.join(directory)).unwrap();
            std::fs::write(fixture.workspace.join(directory).join(literal), "public").unwrap();
        }
        fixture.context.refresh_filenames().unwrap();
        for directory in ["left", "right"] {
            let mut call = message("assistant", "List directory");
            call.tool_calls = Some(vec![ToolCallMsg {
                id: "listing".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "list_dir".into(),
                    arguments: json!({"path":directory}).to_string(),
                    arguments_retrieved_spans: vec![],
                },
            }]);
            let mut result = message("tool", literal);
            result.tool_call_id = Some("listing".into());
            let mut request = PreparedModelRequest::new(
                RequestKind::Headless,
                "fixture",
                "model",
                vec![call, result],
                tools(),
                Default::default(),
            );
            fixture.context.prepare_request(&mut request).unwrap();
            let alias = request.messages[1].content.as_str().unwrap();
            let resolved = fixture
                .context
                .prepare_edit_ticket("read_file", &json!({"path":alias}))
                .unwrap();
            assert_eq!(
                resolved["path"],
                fixture
                    .workspace
                    .join(directory)
                    .join(literal)
                    .to_string_lossy()
                    .as_ref()
            );
        }
    });
}

#[test]
fn filename_unknown_foreign_aliases_cannot_enter_edit_payloads() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        for payload in ["__CA_PATH_foreign__/f001.rs", "__CA_NAME_foreign_999__"] {
            assert!(fixture
                .context
                .prepare_edit_ticket(
                    "write_file",
                    &json!({"path":fixture.alias,"content":payload})
                )
                .is_err());
        }
        assert_eq!(
            std::fs::read_to_string(fixture.path).unwrap(),
            PUBLIC_SOURCE
        );
    });
}

#[test]
fn filename_list_delete_continuation_keeps_history_without_reviving_deleted_path() {
    crate::provider_filter::with_test_filter(&config(), || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let fixture = fixture();
                let directory_alias = fixture.context.mask_known(PRIVATE_DIR).unwrap();
                let workspace = crate::workspace::WorkspaceContext::discover(
                    fixture.workspace.to_str().unwrap(),
                )
                .unwrap();
                super::scope(Some(fixture.context.clone()), async {
                    let rules = [
                        ApprovalRule::AllowTool("list_dir".into()),
                        ApprovalRule::AllowTool("delete_file".into()),
                    ];
                    let broker = ApprovalBroker::new(&workspace, &rules, ApprovalOrigin::Main);
                    let (list, _) = broker
                        .prepare_formatted(
                            "list_dir",
                            &json!({"path":directory_alias}),
                            crate::agent::registry::ToolOrigin::BuiltIn,
                            false,
                            crate::sandbox::SandboxMode::Off,
                        )
                        .await
                        .unwrap();
                    let listing =
                        crate::agent::tools::execute_prepared_tool_in_workspace_policy_owned(
                            "list_dir",
                            &list.arguments,
                            &workspace,
                            5,
                            crate::sandbox::SandboxMode::Off,
                            false,
                            crate::jobs::JobOwner::new(Some(fixture.context.session_id)),
                        )
                        .await;
                    assert!(listing.is_success(), "{}", listing.model_text());
                    assert_eq!(listing.model_text().trim(), PRIVATE_FILE);
                    let mut assistant = message("assistant", "List the supplied directory");
                    assistant.tool_calls = Some(vec![ToolCallMsg {
                        id: "before-delete-list".into(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: "list_dir".into(),
                            arguments: json!({"path":directory_alias}).to_string(),
                            arguments_retrieved_spans: Vec::new(),
                        },
                    }]);
                    let mut output = message("tool", listing.model_text());
                    output.tool_call_id = Some("before-delete-list".into());
                    let messages = vec![
                        message("user", "PublicTask: list then remove the supplied file"),
                        assistant,
                        output,
                    ];
                    let before = serde_json::to_vec(&messages).unwrap();
                    let mut prior = PreparedModelRequest::new(
                        RequestKind::ToolContinuation,
                        "filename-test",
                        "model",
                        messages.clone(),
                        vec![],
                        Default::default(),
                    );
                    fixture.context.prepare_request(&mut prior).unwrap();
                    let (deletion, decision) = broker
                        .prepare_formatted(
                            "delete_file",
                            &json!({"path":fixture.alias}),
                            crate::agent::registry::ToolOrigin::BuiltIn,
                            false,
                            crate::sandbox::SandboxMode::Off,
                        )
                        .await
                        .unwrap();
                    assert_eq!(decision, BrokerDecision::AllowedByRule);
                    let result =
                        crate::agent::tools::execute_prepared_tool_in_workspace_policy_owned(
                            "delete_file",
                            &deletion.arguments,
                            &workspace,
                            5,
                            crate::sandbox::SandboxMode::Off,
                            false,
                            crate::jobs::JobOwner::new(Some(fixture.context.session_id)),
                        )
                        .await;
                    assert!(result.is_success(), "{}", result.model_text());
                    assert!(!fixture.path.exists());
                    let mut after = PreparedModelRequest::new(
                        RequestKind::ToolContinuation,
                        "filename-test",
                        "model",
                        messages.clone(),
                        vec![],
                        Default::default(),
                    );
                    fixture
                        .context
                        .prepare_request(&mut after)
                        .expect("raw historical listing must project after deletion");
                    fixture
                        .context
                        .prepare_request(&mut prior)
                        .expect("already projected historical listing must survive deletion");
                    for request in [after, prior] {
                        let text = request
                            .messages
                            .iter()
                            .find(|message| {
                                message.tool_call_id.as_deref() == Some("before-delete-list")
                            })
                            .unwrap()
                            .content
                            .as_str()
                            .unwrap();
                        assert_eq!(text.trim(), fixture.alias);
                        assert!(!serde_json::to_string(&request.messages)
                            .unwrap()
                            .contains(PRIVATE_FILE));
                    }
                    assert_eq!(
                        serde_json::to_vec(&messages).unwrap(),
                        before,
                        "local listing history changed"
                    );
                    assert!(
                        fixture
                            .context
                            .prepare_edit_ticket("read_file", &json!({"path":fixture.alias}))
                            .is_err(),
                        "historical projection revived a deleted execution target"
                    );
                })
                .await;
            });
    });
}

#[test]
fn filename_edit_content_does_not_rewrite_relative_source_references() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        let original = format!("const NAME: &str = \"{PRIVATE_FILE}\";\n");
        std::fs::write(&fixture.path, &original).unwrap();
        let model_copy = fixture.context.mask_known(&original).unwrap();
        assert!(!model_copy.contains(PRIVATE_FILE));
        assert!(fixture
            .context
            .prepare_edit_ticket(
                "write_file",
                &json!({"path":fixture.alias,"content":model_copy})
            )
            .is_err());
        assert_eq!(std::fs::read_to_string(fixture.path).unwrap(), original);
    });
}

#[test]
fn filename_only_source_and_attachment_contents_remain_readable() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        for (name, source, body) in [
            (
                "SensitiveDocument.html",
                "<main><p data-customer=\"ReadableAttributeCanary\">ReadableBodyCanary</p><script>const amount = 123456;</script></main>\n",
                "ReadableBodyCanary",
            ),
            (
                "SensitiveDocument.txt",
                "OrdinaryReadableTextCanary\namount = 123456\n",
                "OrdinaryReadableTextCanary",
            ),
        ] {
            let path = fixture.workspace.join(name);
            std::fs::write(&path, source).unwrap();
            fixture.context.refresh_filenames().unwrap();
            let projected = fixture.context.project_source(name, source).unwrap();
            assert_eq!(projected.text, source);
            assert!(projected.changes.is_empty());
            assert_eq!(fixture.context.mask_known(body).unwrap(), body);
            assert_eq!(
                fixture.context.state.lock().unwrap().views[name].policy_digest,
                fixture.context.policy_fingerprint(),
                "source view lost its actual policy binding",
            );
            let prefix = format!("Attached {}\n", path.display());
            let original = format!("{prefix}{source}");
            let attachment = crate::attachments::AttachmentSource {
                receipt_id: format!("filename-attachment-{name}"),
                path: path.to_string_lossy().into_owned(),
                source: source.into(),
                original: original.clone(),
                start_line: 1,
                end_line: source.lines().count(),
                prefix,
                suffix: String::new(),
                max_chars: source.chars().count(),
                ranged: false,
            };
            fixture.context.record_attachment(&attachment).unwrap();
            let sent = fixture.context.project_retrieved(
                &original,
                Some(&attachment.receipt_id),
            ).unwrap();
            assert!(sent.ends_with(source), "attachment changed source contents: {sent}");
            assert!(sent.contains(body));
            let alias = sent.lines().next().unwrap().strip_prefix("Attached ").unwrap();
            assert!(alias.starts_with("__CA_PATH_"), "attachment used competing path alias: {alias}");
            assert!(!alias.contains("__CA_PRIVATE_"));
            let resolved = fixture.context.prepare_edit_ticket("read_file", &json!({"path":alias})).unwrap();
            assert_eq!(resolved["path"], path.to_string_lossy().as_ref());
            assert_eq!(fixture.context.mask_known(body).unwrap(), body);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
        }
    });
}

#[test]
fn filename_strict_source_receipts_keep_structured_paths_and_content_privacy() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        let source = "<main>StrictBodyCanary</main>\n";
        let name = "SensitiveStrictDocument.html";
        let path = fixture.workspace.join(name);
        std::fs::write(&path, source).unwrap();
        fixture.context.refresh_filenames().unwrap();
        let mut policy = fixture.context.stored_policy();
        policy.enabled = true;
        fixture.context.save_policy(policy).unwrap();
        assert!(!fixture.context.filename_only());
        let projected = fixture.context.project_source(name, source).unwrap();
        assert!(!projected.text.contains("StrictBodyCanary"));
        let prefix = format!("Attached {}\n", path.display());
        let original = format!("{prefix}{source}");
        let attachment = crate::attachments::AttachmentSource {
            receipt_id: "filename-strict-attachment".into(),
            path: path.to_string_lossy().into_owned(),
            source: source.into(),
            original: original.clone(),
            start_line: 1,
            end_line: 1,
            prefix,
            suffix: String::new(),
            max_chars: 4096,
            ranged: false,
        };
        fixture.context.record_attachment(&attachment).unwrap();
        let sent = fixture
            .context
            .project_retrieved(&original, Some(&attachment.receipt_id))
            .unwrap();
        assert!(!sent.contains("StrictBodyCanary"));
        let alias = sent
            .lines()
            .next()
            .unwrap()
            .strip_prefix("Attached ")
            .unwrap();
        assert!(alias.starts_with("__CA_PATH_"));
        assert!(!alias.contains("__CA_PRIVATE_"));
        let resolved = fixture
            .context
            .prepare_edit_ticket("read_file", &json!({"path":alias}))
            .unwrap();
        assert_eq!(resolved["path"], path.to_string_lossy().as_ref());
        fixture
            .context
            .record_read(path.to_str().unwrap(), source, source, 1, 1, false)
            .unwrap();
        assert_eq!(
            fixture.context.mask_known(path.to_str().unwrap()).unwrap(),
            alias
        );
        assert!(!fixture
            .context
            .mask_known("StrictBodyCanary")
            .unwrap()
            .contains("StrictBodyCanary"));
        for selected_sources in [false, true] {
            if selected_sources {
                let mut policy = fixture.context.stored_policy();
                policy.default_action = super::policy::Action::Allow;
                policy.rules.push(super::policy::Rule {
                    file: name.into(),
                    selector: "/@path".into(),
                    action: super::policy::Action::Hide,
                });
                fixture.context.save_policy(policy).unwrap();
            }
            fixture.context.refresh_classification().unwrap();
            let mut attached = message("user", &original);
            attached.content = ModelContent::text(original.clone()).with_retrieved_spans(vec![
                crate::retrieved_content::RetrievedSpan {
                    start: 0,
                    end: original.len(),
                    source: Some(attachment.receipt_id.clone()),
                },
            ]);
            let mut request = PreparedModelRequest::new(
                RequestKind::Headless,
                "fixture",
                "model",
                vec![attached],
                tools(),
                Default::default(),
            );
            fixture.context.prepare_request(&mut request).unwrap();
            let sent = request.messages[0].content.as_str().unwrap();
            assert!(!sent.contains("StrictBodyCanary"));
            assert!(!sent.contains(name));
            let sent_alias = sent
                .lines()
                .next()
                .unwrap()
                .strip_prefix("Attached ")
                .unwrap();
            assert_eq!(
                sent_alias, alias,
                "classification changed the structured path"
            );
            let resolved = fixture
                .context
                .prepare_edit_ticket("read_file", &json!({"path":sent_alias}))
                .unwrap();
            assert_eq!(resolved["path"], path.to_string_lossy().as_ref());
            assert_eq!(
                fixture.context.mask_known(path.to_str().unwrap()).unwrap(),
                alias
            );
        }
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    });
}

#[test]
fn responses_replay_restores_filename_alias_inside_private_prepare() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        let provider_text = format!("Inspect {}", fixture.alias);
        let mut local = provider_text.clone();
        crate::privacy::scope_sync(Some(fixture.context.clone()), || {
            crate::provider_filter::restore_view_in_place(&mut local);
        });
        assert_ne!(local, provider_text);
        assert!(local.contains(PRIVATE_FILE), "{local}");

        let source = crate::agent::stream::ProviderStateSourceV1::for_test(
            "xai-oauth",
            ProviderProtocol::OpenAiResponses,
        );
        let arguments = r#"{"path":"."}"#;
        let replay = |text: &str| {
            let mut assistant = message("assistant", local.clone());
            assistant.tool_calls = Some(vec![ToolCallMsg {
                id: "call-1".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: arguments.into(),
                    arguments_retrieved_spans: Vec::new(),
                },
            }]);
            assistant.response_items = vec![
                ProviderResponseItem::ResponsesOutput {
                    privacy_prefix_sha256: None,
                    source: source.clone(),
                    output_index: 0,
                    item: json!({
                        "id": "msg_1",
                        "type": "message",
                        "role": "assistant",
                        "content": [{
                            "type": "output_text",
                            "text": text,
                            "annotations": []
                        }],
                        "status": "completed"
                    }),
                },
                ProviderResponseItem::ResponsesOutput {
                    privacy_prefix_sha256: None,
                    source: source.clone(),
                    output_index: 1,
                    item: json!({
                        "id": "fc_1",
                        "type": "function_call",
                        "call_id": "call-1",
                        "name": "read_file",
                        "arguments": arguments,
                        "status": "completed"
                    }),
                },
            ];
            let mut request = PreparedModelRequest::new(
                RequestKind::ToolContinuation,
                "xai-oauth",
                "grok-4.7",
                vec![assistant],
                tools(),
                Default::default(),
            );
            request.runtime_source = Some(source.clone());
            request
        };

        let mut bare = replay(&provider_text);
        fixture
            .context
            .prepare_request(&mut bare)
            .expect("a different spelling must still continue");
        let bare_sent = outgoing_text(&bare);
        assert!(!bare_sent.contains(PRIVATE_FILE), "{bare_sent}");

        let prepared =
            project_private_request(fixture.context.clone(), replay(&provider_text)).unwrap();
        let sent = prepared.messages[0].content.as_str().unwrap();
        assert!(!sent.contains(PRIVATE_FILE), "{sent}");
        assert!(sent.contains(&fixture.alias), "{sent}");

        let different =
            project_private_request(fixture.context.clone(), replay("different provider text"))
                .expect("a different sentence must still continue");
        let different_sent = outgoing_text(&different);
        assert!(!different_sent.contains(PRIVATE_FILE), "{different_sent}");
    });
}

#[test]
fn responses_list_dir_alias_registration_keeps_assistant_replay() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        let extra = fixture.workspace.join("ExtraChildCanary.txt");
        std::fs::write(&extra, "x").unwrap();
        let projected = fixture
            .context
            .project_tool_output("list_dir", r#"{"path":"."}"#, "ExtraChildCanary.txt")
            .unwrap();
        assert!(projected.contains("__CA_PATH_"), "{projected}");
        assert!(!projected.contains("ExtraChildCanary"), "{projected}");

        let provider_text = format!("Inspect {}", fixture.alias);
        let mut local = provider_text.clone();
        crate::privacy::scope_sync(Some(fixture.context.clone()), || {
            crate::provider_filter::restore_view_in_place(&mut local);
        });
        assert!(local.contains(PRIVATE_FILE), "{local}");

        let source = crate::agent::stream::ProviderStateSourceV1::for_test(
            "xai-oauth",
            ProviderProtocol::OpenAiResponses,
        );
        let arguments = r#"{"path":"."}"#;
        let replay = |text: &str| {
            let mut assistant = message("assistant", local.clone());
            assistant.tool_calls = Some(vec![ToolCallMsg {
                id: "call-1".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "list_dir".into(),
                    arguments: arguments.into(),
                    arguments_retrieved_spans: Vec::new(),
                },
            }]);
            assistant.response_items = vec![
                ProviderResponseItem::ResponsesOutput {
                    privacy_prefix_sha256: None,
                    source: source.clone(),
                    output_index: 0,
                    item: json!({
                        "id": "msg_1",
                        "type": "message",
                        "role": "assistant",
                        "content": [{
                            "type": "output_text",
                            "text": text,
                            "annotations": []
                        }],
                        "status": "completed"
                    }),
                },
                ProviderResponseItem::ResponsesOutput {
                    privacy_prefix_sha256: None,
                    source: source.clone(),
                    output_index: 1,
                    item: json!({
                        "id": "fc_1",
                        "type": "function_call",
                        "call_id": "call-1",
                        "name": "list_dir",
                        "arguments": arguments,
                        "status": "completed"
                    }),
                },
            ];
            let mut tool = message("tool", projected.clone());
            tool.tool_call_id = Some("call-1".into());
            let mut request = PreparedModelRequest::new(
                RequestKind::ToolContinuation,
                "xai-oauth",
                "grok-4.7",
                vec![assistant, tool],
                tools(),
                Default::default(),
            );
            request.runtime_source = Some(source.clone());
            request
        };

        let prepared =
            project_private_request(fixture.context.clone(), replay(&provider_text)).unwrap();
        let sent = prepared.messages[0].content.as_str().unwrap();
        assert!(!sent.contains(PRIVATE_FILE), "{sent}");
        assert!(sent.contains(&fixture.alias), "{sent}");

        let different =
            project_private_request(fixture.context.clone(), replay("different provider text"))
                .expect("a different sentence must still continue");
        let different_sent = outgoing_text(&different);
        assert!(!different_sent.contains(PRIVATE_FILE), "{different_sent}");
    });
}

#[test]
fn responses_client_replays_restored_filename_text_on_the_next_request() {
    crate::provider_filter::with_test_filter(&config(), || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let fixture = fixture();
                let provider_text = format!("Inspect {}", fixture.alias);
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
                let first_text = provider_text.clone();
                let server = tokio::spawn(async move {
                    let mut bodies = Vec::new();
                    for text in [first_text, "done".to_string()] {
                        let (mut socket, _) =
                            tokio::time::timeout(Duration::from_secs(6), listener.accept())
                                .await
                                .expect("continuation request was not sent")
                                .unwrap();
                        bodies.push(read_http_body(&mut socket).await);
                        let item = if text == "done" {
                            json!({
                                "id": "msg_done",
                                "type": "message",
                                "role": "assistant",
                                "content": [{"type": "output_text", "text": "done", "annotations": []}],
                                "status": "completed"
                            })
                        } else {
                            json!({
                                "id": "msg_alias",
                                "type": "message",
                                "role": "assistant",
                                "content": [{"type": "output_text", "text": text, "annotations": []}],
                                "status": "completed"
                            })
                        };
                        let function_call = json!({
                            "id": "fc_alias",
                            "type": "function_call",
                            "call_id": "call-alias",
                            "name": "read_file",
                            "arguments": "{\"path\":\".\"}",
                            "status": "completed"
                        });
                        let output = if text == "done" {
                            json!([item])
                        } else {
                            json!([item, function_call])
                        };
                        let event = json!({
                            "type": "response.completed",
                            "response": {
                                "status": "completed",
                                "output": output,
                                "usage": {"input_tokens": 3, "output_tokens": 2, "total_tokens": 5}
                            }
                        });
                        let body = format!("data: {event}\n\n");
                        socket
                            .write_all(
                                format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                    body.len()
                                )
                                .as_bytes(),
                            )
                            .await
                            .unwrap();
                    }
                    bodies
                });
                let client = client(
                    ProviderProtocol::OpenAiResponses,
                    endpoint,
                    fixture.context.clone(),
                );
                let first = PreparedModelRequest::new(
                    RequestKind::Subagent,
                    "filename-test",
                    "model",
                    vec![message("user", "PublicTask: inspect the workspace")],
                    tools(),
                    Default::default(),
                );
                super::scope(Some(fixture.context.clone()), async {
                    let mut stream = client.stream_prepared(&first).await.unwrap();
                    let turn = consume_stream(&mut stream, |_| {}).await.unwrap();
                    assert!(
                        turn.content.contains(PRIVATE_FILE),
                        "assembler left the provider alias in canonical text: {}",
                        turn.content
                    );
                    assert!(!turn.content.contains("__CA_PATH_"), "{}", turn.content);
                    assert_eq!(turn.tool_calls.len(), 1);
                    let call = &turn.tool_calls[0];
                    let mut assistant = message("assistant", turn.content.clone());
                    assistant.content = ModelContent::text(turn.content.clone())
                        .with_retrieved_spans(turn.retrieved_spans.clone());
                    assistant.tool_calls = Some(vec![ToolCallMsg {
                        id: call.id.clone(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: call.name.clone(),
                            arguments: call.arguments.clone(),
                            arguments_retrieved_spans: call.arguments_retrieved_spans.clone(),
                        },
                    }]);
                    assistant.response_items = turn.response_items;
                    let mut tool = message("tool", "listed");
                    tool.tool_call_id = Some(call.id.clone());
                    let continuation = PreparedModelRequest::new(
                        RequestKind::ToolContinuation,
                        "filename-test",
                        "model",
                        vec![
                            message("user", "PublicTask: inspect the workspace"),
                            assistant,
                            tool,
                        ],
                        tools(),
                        Default::default(),
                    );
                    let mut stream = client.stream_prepared(&continuation).await.unwrap();
                    let done = consume_stream(&mut stream, |_| {}).await.unwrap();
                    assert_eq!(done.content, "done");
                })
                .await;
                let bodies = server.await.unwrap();
                assert_eq!(bodies.len(), 2);
                let replayed = bodies[1].to_string();
                assert!(
                    replayed.contains("msg_alias"),
                    "continuation did not replay the issued message: {replayed}"
                );
                assert!(replayed.contains(&fixture.alias), "{replayed}");
                assert_private_names_absent(&bodies[1]);
            });
    });
}

#[test]
fn responses_client_continues_after_uncovered_output_text_and_list_dir() {
    crate::provider_filter::with_test_filter(&config(), || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let fixture = fixture();
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
                let server = tokio::spawn(async move {
                    let mut bodies = Vec::new();
                    let first = concat!(
                        "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"content_index\":0,\"delta\":\"uncovered reasoning prose\"}\n\n",
                        "data: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"content_index\":1,\"delta\":\"extra part\"}\n\n",
                        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[",
                        "{\"id\":\"rs_1\",\"type\":\"reasoning\",\"encrypted_content\":\"opaque-encrypted\",\"status\":\"completed\"},",
                        "{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"status\":\"completed\",\"content\":[{\"type\":\"output_text\",\"text\":\"Listing the workspace.\",\"annotations\":[]}]},",
                        "{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call-1\",\"name\":\"list_dir\",\"arguments\":\"{\\\"path\\\":\\\".\\\"}\",\"status\":\"completed\"}",
                        "],\"usage\":{\"input_tokens\":3,\"output_tokens\":2,\"total_tokens\":5}}}\n\n"
                    );
                    let second = concat!(
                        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[",
                        "{\"id\":\"msg_done\",\"type\":\"message\",\"role\":\"assistant\",\"status\":\"completed\",\"content\":[{\"type\":\"output_text\",\"text\":\"done\",\"annotations\":[]}]}",
                        "],\"usage\":{\"input_tokens\":4,\"output_tokens\":1,\"total_tokens\":5}}}\n\n"
                    );
                    for body in [first, second] {
                        let (mut socket, _) =
                            tokio::time::timeout(Duration::from_secs(6), listener.accept())
                                .await
                                .expect("plan continuation was not sent")
                                .unwrap();
                        bodies.push(read_http_body(&mut socket).await);
                        socket
                            .write_all(
                                format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                    body.len()
                                )
                                .as_bytes(),
                            )
                            .await
                            .unwrap();
                    }
                    bodies
                });
                let client = client(
                    ProviderProtocol::OpenAiResponses,
                    endpoint,
                    fixture.context.clone(),
                );
                let user = message("user", "PublicTask: inspect the workspace");
                let first = PreparedModelRequest::new(
                    RequestKind::Subagent,
                    "filename-test",
                    "model",
                    vec![user.clone()],
                    tools(),
                    Default::default(),
                );
                super::scope(Some(fixture.context.clone()), async {
                    let mut stream = client.stream_prepared(&first).await.unwrap();
                    let turn = consume_stream(&mut stream, |_| {}).await.unwrap();
                    assert_eq!(turn.finish_reason, crate::agent::stream::FinishReason::ToolCalls);
                    assert_eq!(turn.content, "Listing the workspace.");
                    assert!(!turn.content.contains("uncovered"));
                    assert!(!turn.content.contains("extra part"));
                    assert_eq!(turn.tool_calls.len(), 1);
                    assert_eq!(turn.tool_calls[0].name, "list_dir");
                    let call = &turn.tool_calls[0];
                    let mut assistant = message("assistant", turn.content.clone());
                    assistant.content = ModelContent::text(turn.content.clone())
                        .with_retrieved_spans(turn.retrieved_spans.clone());
                    assistant.tool_calls = Some(vec![ToolCallMsg {
                        id: call.id.clone(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: call.name.clone(),
                            arguments: call.arguments.clone(),
                            arguments_retrieved_spans: call.arguments_retrieved_spans.clone(),
                        },
                    }]);
                    assistant.response_items = turn.response_items;
                    let mut tool = message("tool", format!("{PRIVATE_DIR}/"));
                    tool.tool_call_id = Some(call.id.clone());
                    let continuation = PreparedModelRequest::new(
                        RequestKind::ToolContinuation,
                        "filename-test",
                        "model",
                        vec![user, assistant, tool],
                        tools(),
                        Default::default(),
                    );
                    let mut stream = client
                        .stream_prepared(&continuation)
                        .await
                        .expect("continuation prepare failed after list_dir");
                    let done = consume_stream(&mut stream, |_| {}).await.unwrap();
                    assert_eq!(done.content, "done");
                })
                .await;
                let bodies = server.await.unwrap();
                assert_eq!(bodies.len(), 2, "the plan continuation was not sent");
                let replayed = bodies[1].to_string();
                assert!(
                    replayed.contains("Listing the workspace."),
                    "continuation replayed the uncovered deltas: {replayed}"
                );
                assert!(!replayed.contains("uncovered reasoning prose"), "{replayed}");
                assert!(!replayed.contains("extra part"), "{replayed}");
                assert!(replayed.contains("msg_1"), "{replayed}");
            });
    });
}

#[test]
fn responses_plan_continuation_restores_filename_alias_after_list_dir() {
    crate::provider_filter::with_test_filter(&config(), || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let fixture = fixture();
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
                let alias = fixture.alias.clone();
                let server = tokio::spawn(async move {
                    let mut bodies = Vec::new();
                    let first = format!(
                        "data: {{\"type\":\"response.output_text.delta\",\"output_index\":1,\"content_index\":0,\"delta\":{alias_json}}}\n\n\
                         data: {{\"type\":\"response.completed\",\"response\":{{\"status\":\"completed\",\"output\":[\
                         {{\"id\":\"rs_1\",\"type\":\"reasoning\",\"encrypted_content\":\"opaque\",\"status\":\"completed\"}},\
                         {{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"status\":\"completed\",\"content\":[{{\"type\":\"output_text\",\"text\":{alias_json},\"annotations\":[]}}]}},\
                         {{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call-1\",\"name\":\"list_dir\",\"arguments\":\"{{\\\"path\\\":\\\".\\\"}}\",\"status\":\"completed\"}}\
                         ],\"usage\":{{\"input_tokens\":3,\"output_tokens\":2,\"total_tokens\":5}}}}}}\n\n",
                        alias_json = serde_json::to_string(&alias).unwrap(),
                    );
                    let second = concat!(
                        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[",
                        "{\"id\":\"msg_done\",\"type\":\"message\",\"role\":\"assistant\",\"status\":\"completed\",\"content\":[{\"type\":\"output_text\",\"text\":\"done\",\"annotations\":[]}]}",
                        "],\"usage\":{\"input_tokens\":4,\"output_tokens\":1,\"total_tokens\":5}}}\n\n"
                    );
                    for body in [first, second.to_string()] {
                        let (mut socket, _) =
                            tokio::time::timeout(Duration::from_secs(6), listener.accept())
                                .await
                                .expect("continuation was not sent")
                                .unwrap();
                        bodies.push(read_http_body(&mut socket).await);
                        socket
                            .write_all(
                                format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                    body.len()
                                )
                                .as_bytes(),
                            )
                            .await
                            .unwrap();
                    }
                    bodies
                });
                let store = RequestStore::default();
                store.set_privacy_context(Some(fixture.context.clone()));
                let client = client_with_store(
                    "filename-test",
                    ProviderProtocol::OpenAiResponses,
                    endpoint,
                    store,
                );
                let user = message("user", "PublicTask: inspect the workspace");
                let first = PreparedModelRequest::new(
                    RequestKind::Subagent,
                    "filename-test",
                    "model",
                    vec![user.clone()],
                    tools(),
                    Default::default(),
                );
                super::scope(Some(fixture.context.clone()), async {
                    let mut stream = client.stream_prepared(&first).await.unwrap();
                    let turn = consume_stream(&mut stream, |_| {}).await.unwrap();
                    assert!(
                        turn.content.contains(PRIVATE_FILE),
                        "stored assistant text stayed at the provider spelling: {}",
                        turn.content
                    );
                    assert!(!turn.content.contains("__CA_PATH_"), "{}", turn.content);
                    let call = &turn.tool_calls[0];
                    let mut assistant = message("assistant", turn.content.clone());
                    assistant.content = ModelContent::text(turn.content.clone())
                        .with_retrieved_spans(turn.retrieved_spans.clone());
                    assistant.tool_calls = Some(vec![ToolCallMsg {
                        id: call.id.clone(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: call.name.clone(),
                            arguments: call.arguments.clone(),
                            arguments_retrieved_spans: call.arguments_retrieved_spans.clone(),
                        },
                    }]);
                    assistant.response_items = turn.response_items;
                    let mut tool = message("tool", format!("{PRIVATE_DIR}/"));
                    tool.tool_call_id = Some(call.id.clone());
                    let continuation = PreparedModelRequest::new(
                        RequestKind::ToolContinuation,
                        "filename-test",
                        "model",
                        vec![user, assistant, tool],
                        tools(),
                        Default::default(),
                    );
                    let mut stream = client
                        .stream_prepared(&continuation)
                        .await
                        .expect("continuation lost the filename scope");
                    let done = consume_stream(&mut stream, |_| {}).await.unwrap();
                    assert_eq!(done.content, "done");
                })
                .await;
                let bodies = server.await.unwrap();
                assert_eq!(bodies.len(), 2);
                let replayed = bodies[1].to_string();
                assert!(replayed.contains(&fixture.alias), "{replayed}");
                assert!(!replayed.contains(PRIVATE_FILE), "{replayed}");
            });
    });
}

#[test]
fn responses_plan_author_continuation_accepts_session_privacy_spelling_after_list_dir() {
    crate::provider_filter::with_test_filter(&config(), || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let fixture = fixture();
                let secret = "CustomerSecretCanaryValue".repeat(4);
                let _url = "https://private-customer-dataset-canary.example/datasets";
                let task = fixture
                    .context
                    .with_task_filter(crate::task_filter::Policy {
                        words: vec![secret.clone()],
                        ..Default::default()
                    })
                    .unwrap();
                let filter = task.task_filter().expect("task filter");
                let namespace = filter.task_record().unwrap().task_id.simple().to_string();
                let word_alias = format!("__CA_TASK_{namespace}_WORD_0__");
                filter
                    .task_url_map()
                    .lock()
                    .unwrap()
                    .push("https://private-customer-dataset-canary.example".into());
                let url_alias = "https://ca-r01.invalid/datasets";
                task.refresh_filenames().unwrap();
                let file_alias = task
                    .mask_known(&fixture.path.to_string_lossy())
                    .unwrap();
                assert!(
                    file_alias.contains("__CA_PATH_") || file_alias.contains("__CA_NAME_"),
                    "{file_alias}"
                );
                assert_ne!(file_alias, fixture.path.to_string_lossy());
                let provider_text =
                    format!("Inspect {file_alias}. Note {word_alias}. See {url_alias}.");
                let mut process_only = provider_text.clone();
                crate::provider_filter::restore_view_with_spans(&mut process_only);
                assert!(
                    !process_only.contains(&secret) && !process_only.contains(PRIVATE_FILE),
                    "process filter alone restored the session spelling: {process_only}"
                );

                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
                let first_text = provider_text.clone();
                let server = tokio::spawn(async move {
                    let mut bodies = Vec::new();
                    let completed = |text: String, call: bool| {
                        let mut output = vec![
                            json!({
                                "id": "rs_1",
                                "type": "reasoning",
                                "encrypted_content": "opaque",
                                "status": "completed"
                            }),
                            json!({
                                "id": "msg_1",
                                "type": "message",
                                "role": "assistant",
                                "status": "completed",
                                "content": [{
                                    "type": "output_text",
                                    "text": text,
                                    "annotations": []
                                }]
                            }),
                        ];
                        if call {
                            output.push(json!({
                                "id": "fc_1",
                                "type": "function_call",
                                "call_id": "call-1",
                                "name": "list_dir",
                                "arguments": "{\"path\":\".\"}",
                                "status": "completed"
                            }));
                        }
                        json!({
                            "type": "response.completed",
                            "response": {
                                "status": "completed",
                                "output": output,
                                "usage": {"input_tokens": 3, "output_tokens": 2, "total_tokens": 5}
                            }
                        })
                    };
                    let first = format!(
                        "data: {}\n\ndata: {}\n\n",
                        json!({
                            "type": "response.output_text.delta",
                            "output_index": 1,
                            "content_index": 0,
                            "delta": first_text
                        }),
                        completed(first_text.clone(), true)
                    );
                    let second = format!(
                        "data: {}\n\n",
                        completed("done".into(), false)
                    );
                    for body in [first, second] {
                        let (mut socket, _) =
                            tokio::time::timeout(Duration::from_secs(6), listener.accept())
                                .await
                                .expect("continuation was not sent")
                                .unwrap();
                        bodies.push(read_http_body(&mut socket).await);
                        socket
                            .write_all(
                                format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                    body.len()
                                )
                                .as_bytes(),
                            )
                            .await
                            .unwrap();
                    }
                    bodies
                });
                let store = RequestStore::default();
                store.set_privacy_context(Some(fixture.context.clone()));
                let client = client_with_store(
                    "filename-test",
                    ProviderProtocol::OpenAiResponses,
                    endpoint,
                    store,
                );
                let user = message("user", "PublicTask: inspect the workspace");
                let first = PreparedModelRequest::new(
                    RequestKind::Subagent,
                    "filename-test",
                    "model",
                    vec![user.clone()],
                    tools(),
                    Default::default(),
                );
                super::scope(Some(task.clone()), async {
                    let mut stream = client.stream_prepared(&first).await.unwrap();
                    let turn = consume_stream(&mut stream, |_| {}).await.unwrap();
                    assert!(
                        turn.content.contains(&secret),
                        "stored text is not the restored selected value: {}",
                        turn.content
                    );
                    assert!(
                        turn.content.contains("private-customer-dataset-canary.example"),
                        "stored text is not the restored URL: {}",
                        turn.content
                    );
                    assert!(
                        turn.content.contains(PRIVATE_FILE),
                        "stored text is not the restored filename: {}",
                        turn.content
                    );
                    assert!(
                        turn.content.len() > provider_text.len() + 40,
                        "stored len {} provider len {}",
                        turn.content.len(),
                        provider_text.len()
                    );
                    let registered = task
                        .project_tool_output(
                            "list_dir",
                            &json!({"path": format!("{PRIVATE_DIR}/")}).to_string(),
                            PRIVATE_FILE,
                        )
                        .unwrap();
                    assert!(
                        registered.contains("__CA_PATH_"),
                        "list_dir did not register the stored name: {registered}"
                    );
                    assert!(
                        turn.content.contains(PRIVATE_FILE),
                        "registered name is absent from the stored assistant text"
                    );
                    let call = &turn.tool_calls[0];
                    let mut assistant = message("assistant", turn.content.clone());
                    assistant.content = ModelContent::text(turn.content.clone())
                        .with_retrieved_spans(turn.retrieved_spans.clone());
                    assistant.tool_calls = Some(vec![ToolCallMsg {
                        id: call.id.clone(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: call.name.clone(),
                            arguments: call.arguments.clone(),
                            arguments_retrieved_spans: call.arguments_retrieved_spans.clone(),
                        },
                    }]);
                    assistant.response_items = turn.response_items.clone();
                    let mut tool = message("tool", registered);
                    tool.tool_call_id = Some(call.id.clone());
                    let continuation = PreparedModelRequest::new(
                        RequestKind::ToolContinuation,
                        "filename-test",
                        "model",
                        vec![user.clone(), assistant.clone(), tool.clone()],
                        tools(),
                        Default::default(),
                    );
                    let mut stream = client.stream_prepared(&continuation).await.unwrap_or_else(|error| {
                        panic!("plan continuation rejected the session spelling: {error}");
                    });
                    let done = consume_stream(&mut stream, |_| {}).await.unwrap();
                    assert_eq!(done.content, "done");
                })
                .await;
                let bodies = server.await.unwrap();
                assert_eq!(bodies.len(), 2, "the plan continuation was not sent");
                let replayed = bodies[1].to_string();
                assert!(replayed.contains(&word_alias), "{replayed}");
                assert!(replayed.contains(&file_alias), "{replayed}");
                assert!(replayed.contains(url_alias), "{replayed}");
                assert!(!replayed.contains(&secret), "{replayed}");
                assert!(!replayed.contains(PRIVATE_FILE), "{replayed}");
                assert!(!replayed.contains("private-customer-dataset-canary.example"), "{replayed}");
            });
    });
}

#[test]
fn responses_plan_continuation_sends_home_tilde_list_dir() {
    crate::provider_filter::with_test_filter(&config(), || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let fixture = fixture();
                let ssh = fixture.workspace.join(".ssh");
                std::fs::create_dir_all(&ssh).unwrap();
                std::fs::write(ssh.join("SshPlanCanary"), "host canary\n").unwrap();
                fixture
                    .context
                    .set_filename_home_for_test(fixture.context.workspace().to_path_buf());
                let arguments = r#"{"path":"~/.ssh"}"#;
                let listing = "SshPlanCanary";
                let projected = fixture
                    .context
                    .project_tool_output("list_dir", arguments, listing)
                    .unwrap_or_else(|error| {
                        panic!("list_dir ~/.ssh was withheld ({error}); the plan continuation cannot send it");
                    });
                assert!(
                    projected.contains("__CA_PATH_") || projected.contains("__CA_NAME_"),
                    "{projected}"
                );
                assert!(!projected.contains("SshPlanCanary"), "{projected}");
                assert!(!projected.contains(".ssh"), "{projected}");
                let mut withheld_call = message("assistant", "list");
                withheld_call.tool_calls = Some(vec![ToolCallMsg {
                    id: "call-withheld".into(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: "list_dir".into(),
                        arguments: arguments.into(),
                        arguments_retrieved_spans: Vec::new(),
                    },
                }]);
                let mut withheld = message("tool", "[listing withheld]");
                withheld.tool_call_id = Some("call-withheld".into());
                withheld.content = withheld
                    .content
                    .with_tool_result_status(true);
                let mut withheld_request = PreparedModelRequest::new(
                    RequestKind::ToolContinuation,
                    "filename-test",
                    "model",
                    vec![withheld_call, withheld],
                    tools(),
                    Default::default(),
                );
                fixture
                    .context
                    .prepare_request(&mut withheld_request)
                    .unwrap_or_else(|error| {
                        panic!("withheld listing blocked the next request: {error}");
                    });
                assert_eq!(
                    withheld_request.messages[1].content.as_str(),
                    Some("[listing withheld]")
                );

                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
                let server = tokio::spawn(async move {
                    let mut bodies = Vec::new();
                    let completed = |text: &str, call: bool| {
                        let mut output = vec![json!({
                            "id": "msg_1",
                            "type": "message",
                            "role": "assistant",
                            "status": "completed",
                            "content": [{
                                "type": "output_text",
                                "text": text,
                                "annotations": []
                            }]
                        })];
                        if call {
                            output.push(json!({
                                "id": "fc_1",
                                "type": "function_call",
                                "call_id": "call-1",
                                "name": "list_dir",
                                "arguments": arguments,
                                "status": "completed"
                            }));
                        }
                        json!({
                            "type": "response.completed",
                            "response": {
                                "status": "completed",
                                "output": output,
                                "usage": {"input_tokens": 3, "output_tokens": 2, "total_tokens": 5}
                            }
                        })
                    };
                    let first = format!("data: {}\n\n", completed("Listing ssh config.", true));
                    let second = format!("data: {}\n\n", completed("done", false));
                    for body in [first, second] {
                        let (mut socket, _) =
                            tokio::time::timeout(Duration::from_secs(6), listener.accept())
                                .await
                                .expect("continuation was not sent")
                                .unwrap();
                        bodies.push(read_http_body(&mut socket).await);
                        socket
                            .write_all(
                                format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                    body.len()
                                )
                                .as_bytes(),
                            )
                            .await
                            .unwrap();
                    }
                    bodies
                });
                let store = RequestStore::default();
                store.set_privacy_context(Some(fixture.context.clone()));
                let client = client_with_store(
                    "filename-test",
                    ProviderProtocol::OpenAiResponses,
                    endpoint,
                    store,
                );
                let user = message("user", "ssh bsd");
                let first = PreparedModelRequest::new(
                    RequestKind::Subagent,
                    "filename-test",
                    "model",
                    vec![user.clone()],
                    tools(),
                    Default::default(),
                );
                super::scope(Some(fixture.context.clone()), async {
                    let mut stream = client.stream_prepared(&first).await.unwrap();
                    let turn = consume_stream(&mut stream, |_| {}).await.unwrap();
                    let call = &turn.tool_calls[0];
                    assert_eq!(call.name, "list_dir");
                    let mut assistant = message("assistant", turn.content.clone());
                    assistant.content = ModelContent::text(turn.content.clone())
                        .with_retrieved_spans(turn.retrieved_spans.clone());
                    assistant.tool_calls = Some(vec![ToolCallMsg {
                        id: call.id.clone(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: call.name.clone(),
                            arguments: call.arguments.clone(),
                            arguments_retrieved_spans: call.arguments_retrieved_spans.clone(),
                        },
                    }]);
                    assistant.response_items = turn.response_items;
                    let stored = match fixture.context.project_tool_output(
                        "list_dir",
                        &call.arguments,
                        listing,
                    ) {
                        Ok(projected) => projected,
                        Err(_) => "[listing withheld]".into(),
                    };
                    let mut tool = message("tool", stored);
                    tool.tool_call_id = Some(call.id.clone());
                    let continuation = PreparedModelRequest::new(
                        RequestKind::ToolContinuation,
                        "filename-test",
                        "model",
                        vec![user, assistant, tool],
                        tools(),
                        Default::default(),
                    );
                    let mut stream = client.stream_prepared(&continuation).await.unwrap_or_else(|error| {
                        panic!("plan continuation blocked the tilde listing: {error}");
                    });
                    let done = consume_stream(&mut stream, |_| {}).await.unwrap();
                    assert_eq!(done.content, "done");
                })
                .await;
                let bodies = server.await.unwrap();
                assert_eq!(bodies.len(), 2, "the plan continuation was not sent");
                let replayed = bodies[1].to_string();
                assert!(!replayed.contains("SshPlanCanary"), "{replayed}");
                assert!(!replayed.contains(".ssh"), "{replayed}");
                assert!(
                    replayed.contains("__CA_PATH_") || replayed.contains("__CA_NAME_"),
                    "{replayed}"
                );
            });
    });
}

#[test]
fn responses_reasoning_summary_text_is_projected_before_wire_validation() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        let private_path = format!("{PRIVATE_DIR}/{PRIVATE_FILE}");
        let summary = format!("Need to inspect {private_path} before listing.");
        assert!(
            fixture.context.mask_known(&summary).unwrap() != summary,
            "fixture must treat the reasoning summary as private"
        );
        let mut source = crate::agent::stream::ProviderStateSourceV1::for_test(
            "xai-oauth",
            ProviderProtocol::OpenAiResponses,
        );
        source.privacy_policy_digest = Some(fixture.context.replay_policy_digest());
        let assistant_text = "Listing the workspace.";
        let mut assistant = message("assistant", assistant_text);
        assistant.response_items = vec![
            ProviderResponseItem::ResponsesOutput {
                privacy_prefix_sha256: None,
                source: source.clone(),
                output_index: 0,
                item: json!({
                    "id": "rs_1",
                    "type": "reasoning",
                    "summary": [{"type": "summary_text", "text": summary}],
                    "encrypted_content": "opaque-encrypted",
                    "status": "completed"
                }),
            },
            ProviderResponseItem::ResponsesOutput {
                privacy_prefix_sha256: None,
                source: source.clone(),
                output_index: 1,
                item: json!({
                    "id": "msg_1",
                    "type": "message",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": assistant_text,
                        "annotations": []
                    }],
                    "status": "completed"
                }),
            },
        ];
        let mut request = PreparedModelRequest::new(
            RequestKind::Subagent,
            "xai-oauth",
            "grok-4.7",
            vec![
                message("user", "PublicTask: inspect the workspace"),
                assistant,
            ],
            tools(),
            Default::default(),
        );
        request.runtime_source = Some(source);
        let prepared = project_private_request(fixture.context.clone(), request)
            .expect("prepare should keep grok reasoning replay");
        let items = prepared.messages[1]
            .response_items
            .iter()
            .map(|item| match item {
                ProviderResponseItem::ResponsesOutput { item, .. } => item.clone(),
                other => panic!("unexpected response item: {other:?}"),
            })
            .collect::<Vec<_>>();
        let body = serde_json::to_vec(&json!({ "input": items })).unwrap();
        let revision = prepared
            .privacy_revision
            .expect("prepared request needs a privacy revision");
        fixture
            .context
            .validate_wire(&body, revision, &fixture.context.policy_fingerprint())
            .expect("reasoning summary must be privacy-safe on the wire");
        let sent_summary = items[0]["summary"][0]["text"].as_str().unwrap();
        assert!(
            !sent_summary.contains(PRIVATE_DIR),
            "summary still has the private directory: {sent_summary}"
        );
        assert!(
            !sent_summary.contains(PRIVATE_FILE),
            "summary still has the private filename: {sent_summary}"
        );
        assert!(
            sent_summary.contains("__CA_PATH_"),
            "summary was not projected to a path alias: {sent_summary}"
        );
        assert_eq!(items[0]["encrypted_content"], "opaque-encrypted");
    });
}

#[test]
fn responses_message_content_text_is_projected_before_wire_validation() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        std::fs::write(fixture.workspace.join("n.rs"), "fn n() {}\n").unwrap();
        fixture.context.refresh_filenames().unwrap();
        let sentence = "Read n.rs next.";
        assert_ne!(
            fixture.context.mask_known(sentence).unwrap(),
            sentence,
            "fixture must treat the short filename as private"
        );
        let mut source = crate::agent::stream::ProviderStateSourceV1::for_test(
            "xai-oauth",
            ProviderProtocol::OpenAiResponses,
        );
        source.privacy_policy_digest = Some(fixture.context.replay_policy_digest());
        let mut assistant = message("assistant", sentence);
        assistant.response_items = vec![
            ProviderResponseItem::ResponsesOutput {
                privacy_prefix_sha256: None,
                source: source.clone(),
                output_index: 0,
                item: json!({
                    "id": "rs_1",
                    "type": "reasoning",
                    "summary": [],
                    "encrypted_content": "opaque-encrypted",
                    "status": "completed"
                }),
            },
            ProviderResponseItem::ResponsesOutput {
                privacy_prefix_sha256: None,
                source: source.clone(),
                output_index: 1,
                item: json!({
                    "id": "msg_1",
                    "type": "message",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": sentence,
                        "annotations": []
                    }],
                    "status": "completed"
                }),
            },
        ];
        let mut request = PreparedModelRequest::new(
            RequestKind::Subagent,
            "xai-oauth",
            "grok-4.7",
            vec![
                message("user", "PublicTask: inspect the workspace"),
                assistant,
            ],
            tools(),
            Default::default(),
        );
        request.runtime_source = Some(source);
        let prepared = project_private_request(fixture.context.clone(), request)
            .expect("prepare should keep grok message replay");
        let items = prepared.messages[1]
            .response_items
            .iter()
            .map(|item| match item {
                ProviderResponseItem::ResponsesOutput { item, .. } => item.clone(),
                other => panic!("unexpected response item: {other:?}"),
            })
            .collect::<Vec<_>>();
        let body = serde_json::to_vec(&json!({ "input": items })).unwrap();
        let revision = prepared
            .privacy_revision
            .expect("prepared request needs a privacy revision");
        fixture
            .context
            .validate_wire(&body, revision, &fixture.context.policy_fingerprint())
            .expect("message content must be privacy-safe on the wire");
        let sent = items[1]["content"][0]["text"].as_str().unwrap();
        assert!(
            !sent.contains("n.rs"),
            "message content still has the short filename: {sent}"
        );
        assert!(
            sent.contains("__CA_PATH_") || sent.contains("__CA_NAME_"),
            "message content was not projected to an alias: {sent}"
        );
        assert_eq!(items[0]["encrypted_content"], "opaque-encrypted");
    });
}

#[test]
fn responses_replay_accepts_stale_filename_alias_after_tools() {
    crate::provider_filter::with_test_filter(&config(), || {
        let fixture = fixture();
        let private_path = format!("{PRIVATE_DIR}/{PRIVATE_FILE}");
        let canonical = format!("Need to inspect {private_path} before the next tool.");
        let current = fixture.context.mask_known(&canonical).unwrap();
        assert_ne!(current, canonical);
        let stale = current.replacen("__CA_PATH_", "__CA_PATH_stale_", 1);
        assert_ne!(
            stale, current,
            "stale alias must differ from the current spelling"
        );
        let item_text = stale;
        let source = crate::agent::stream::ProviderStateSourceV1::for_test(
            "xai-oauth",
            ProviderProtocol::OpenAiResponses,
        );
        let replay = |text: &str, item: &str| {
            let mut assistant = message("assistant", text);
            assistant.response_items = vec![ProviderResponseItem::ResponsesOutput {
                privacy_prefix_sha256: None,
                source: source.clone(),
                output_index: 1,
                item: json!({
                    "id": "msg_1",
                    "type": "message",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": item,
                        "annotations": []
                    }],
                    "status": "completed"
                }),
            }];
            let mut request = PreparedModelRequest::new(
                RequestKind::ToolContinuation,
                "xai-oauth",
                "grok-4.7",
                vec![
                    message("user", "PublicTask: inspect the workspace"),
                    assistant,
                ],
                tools(),
                Default::default(),
            );
            request.runtime_source = Some(source.clone());
            request
        };
        project_private_request(fixture.context.clone(), replay(&canonical, &item_text))
            .expect("stale filename alias must still continue");
        let alias = fixture.context.mask_known(&private_path).unwrap();
        let aliased = format!("Need to inspect {alias} before the next tool.");
        let named = format!("Need to inspect {private_path} before the next tool.");
        project_private_request(fixture.context.clone(), replay(&aliased, &named))
            .expect("provider filename must match the stored alias spelling");
        let continued = project_private_request(
            fixture.context.clone(),
            replay("A different sentence about the workspace.", &item_text),
        )
        .expect("a different sentence must still continue");
        let continued_sent = outgoing_text(&continued);
        assert!(!continued_sent.contains(&private_path), "{continued_sent}");
    });
}

fn outgoing_text(request: &PreparedModelRequest) -> String {
    let mut text = String::new();
    for message in &request.messages {
        if let Some(content) = message.content.as_str() {
            text.push_str(content);
        }
        for item in &message.response_items {
            if let ProviderResponseItem::ResponsesOutput { item, .. } = item {
                text.push_str(&item.to_string());
            }
        }
    }
    text
}
