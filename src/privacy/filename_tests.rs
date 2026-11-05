//! Held-out filename boundary checks through real client and native-tool entry points.
use super::Context;
use crate::agent::client::{
    AgentClient, ChatMessage, FunctionCall, ProviderCallPolicy, ToolCallMsg, ToolDef, ToolFunction,
};
use crate::agent::stream::{consume_stream, ProviderError};
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
    client_with_store(protocol, endpoint, store)
}

fn client_with_store(
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
    AgentClient::with_call_policy("filename-test", &provider, "model", policy, store)
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
                    let client =
                        client_with_store(ProviderProtocol::ChatCompletions, endpoint, store);
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
