//! Custom protocol names must not bypass an already-private identity.
use super::Context;
use crate::agent::client::{
    AgentClient, ChatMessage, FunctionCall, ProviderCallPolicy, ToolCallMsg, ToolDef, ToolFunction,
};
use crate::agent::stream::ProviderError;
use crate::agent::ProviderProtocol;
use crate::config::{PromptCacheMode, ProviderConfig, ProviderTimeoutConfig};
use crate::image_content::ModelContent;
use crate::request::RequestStore;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const PRIVATE: &str = "PrivateCustomToolCanary";

fn context() -> (tempfile::TempDir, Arc<Context>) {
    let root = tempfile::tempdir().unwrap();
    let context = Context::open(
        root.path().into(),
        root.path().into(),
        uuid::Uuid::new_v4(),
        None,
    )
    .unwrap();
    context.allow_ephemeral();
    let mut policy = context.policy();
    policy.enabled = true;
    context.save_policy(policy).unwrap();
    context.project_source("private.txt", PRIVATE).unwrap();
    (root, context)
}

fn message(role: &str, text: &str) -> ChatMessage {
    ChatMessage {
        role: role.into(),
        content: ModelContent::text(text),
        tool_calls: None,
        tool_call_id: None,
        response_items: Vec::new(),
    }
}

#[tokio::test]
async fn privacy_private_custom_tool_and_call_names_block_all_protocols_before_socket() {
    let (_root, context) = context();
    for protocol in [
        ProviderProtocol::ChatCompletions,
        ProviderProtocol::OpenAiResponses,
        ProviderProtocol::AnthropicMessages,
        ProviderProtocol::GoogleGenerateContent,
    ] {
        for call_name in [false, true] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let store = RequestStore::default();
            store.set_privacy_context(Some(context.clone()));
            let provider = ProviderConfig {
                base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
                api_key: String::new(),
                protocol,
                models: vec!["model".into()],
                timeout: ProviderTimeoutConfig::default(),
                google: None,
                tool_schema_policy: Default::default(),
                prompt_cache_mode: PromptCacheMode::Implicit,
            };
            let policy = ProviderCallPolicy {
                connect_timeout: Duration::from_secs(1),
                headers_timeout: Duration::from_secs(1),
                first_event_timeout: Duration::from_secs(1),
                no_progress_timeout: Duration::from_secs(1),
                max_attempts: 1,
                retry_delay: Duration::from_millis(1),
                max_retry_after: Duration::from_millis(1),
            };
            let client =
                AgentClient::with_call_policy("privacy-test", &provider, "model", policy, store)
                    .with_filter_for_test(Arc::new(
                        crate::provider_filter::ProviderFilter::compile(&Default::default())
                            .unwrap(),
                    ));
            let mut messages = vec![message("user", "Public task")];
            let mut tools = Vec::new();
            if call_name {
                let mut assistant = message("assistant", "Public response");
                assistant.tool_calls = Some(vec![ToolCallMsg {
                    id: "public-call-id".into(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: PRIVATE.into(),
                        arguments: "{}".into(),
                        arguments_retrieved_spans: Vec::new(),
                    },
                }]);
                messages.push(assistant);
            } else {
                tools.push(ToolDef {
                    def_type: "function".into(),
                    function: ToolFunction {
                        name: PRIVATE.into(),
                        description: "Public contract".into(),
                        parameters: json!({"type":"object"}),
                    },
                });
            }
            let result = client.chat_stream(&messages, &tools).await;
            assert!(
                matches!(result, Err(ProviderError::PrivacyBlocked { .. })),
                "{protocol:?}, call={call_name}: {result:?}"
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(25), listener.accept())
                    .await
                    .is_err(),
                "blocked name opened a socket"
            );
        }
    }
}

#[test]
fn privacy_final_name_controls_are_bound_to_real_protocol_envelopes() {
    let (_root, context) = context();
    context.project_source("builtin.txt", "read_file").unwrap();
    let revision = context.policy().revision;
    let fingerprint = context.policy_fingerprint();
    for body in [
        json!({"tools":[{"type":"function","function":{"name":"read_file","parameters":{"type":"object"}}}]}),
        json!({"tools":[{"type":"function","name":"read_file","parameters":{"type":"object"}}]}),
        json!({"tools":[{"name":"read_file","input_schema":{"type":"object"}}]}),
        json!({"tools":[{"functionDeclarations":[{"name":"read_file","parameters":{"type":"object"}}]}]}),
    ] {
        context
            .validate_wire(&serde_json::to_vec(&body).unwrap(), revision, &fingerprint)
            .unwrap();
    }
    for body in [
        json!({"data":{"name":PRIVATE}}),
        json!({"data":{"name":"read_file"}}),
        json!({"input":[{"type":"function_call","name":PRIVATE,"arguments":"{}"}]}),
        json!({"messages":[{"role":"assistant","content":[{"type":"tool_use","id":"public-id","name":PRIVATE,"input":{}}]}]}),
        json!({"contents":[{"role":"model","parts":[{"functionCall":{"name":PRIVATE,"args":{}}}]}]}),
    ] {
        assert!(
            context
                .validate_wire(&serde_json::to_vec(&body).unwrap(), revision, &fingerprint)
                .is_err(),
            "private name escaped: {body}"
        );
    }
}
