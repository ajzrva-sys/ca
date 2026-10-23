use super::*;
use crate::agent::stream::FinishReason;
use crate::app::{Message, Role, ToolCall, ToolStatus};
use crate::config::{ProviderTimeoutConfig, ReasoningEffort, ReasoningRequestFormat};
use std::time::Duration;

fn fixed_time() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-08-01T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc)
}

fn user(content: &str) -> Message {
    Message {
        role: Role::User,
        content: content.to_string(),
        tool_calls: None,
        thinking: None,
        response_items: Vec::new(),
        timestamp: fixed_time(),
    }
}

fn system(content: &str) -> Message {
    Message {
        role: Role::System,
        content: content.to_string(),
        tool_calls: None,
        thinking: None,
        response_items: Vec::new(),
        timestamp: fixed_time(),
    }
}

fn call(id: &str, status: ToolStatus, output: Option<&str>) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: "read_file".to_string(),
        arguments: r#"{"path":"input.txt"}"#.to_string(),
        status,
        output: output.map(str::to_string),
    }
}

fn assistant_with_calls(calls: Vec<ToolCall>) -> Message {
    Message {
        role: Role::Assistant,
        content: String::new(),
        tool_calls: Some(calls),
        thinking: None,
        response_items: Vec::new(),
        timestamp: fixed_time(),
    }
}

fn roles(messages: &[ChatMessage]) -> Vec<&str> {
    messages
        .iter()
        .map(|message| message.role.as_str())
        .collect()
}

fn official_contract(
    provider: &str,
    protocol: crate::agent::ProviderProtocol,
    model: &str,
) -> crate::provider_contract::FirstPartyContract {
    let base_url = match provider {
        "openai" => "https://api.openai.com/v1",
        "openai-codex" => "https://chatgpt.com/backend-api/codex",
        "mimo" => "https://api.xiaomimimo.com/v1",
        "deepseek" => "https://api.deepseek.com/v1",
        _ => panic!("unsupported first-party test provider"),
    };
    crate::provider_contract::resolve(
        provider,
        &ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: base_url.into(),
            api_key: String::new(),
            protocol,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec![model.into()],
            timeout: ProviderTimeoutConfig::default(),
        },
        model,
    )
    .unwrap()
}

#[test]
fn segmented_prompt_serializes_checkpoint_before_the_dynamic_cache_barrier() {
    let assembly = AgentClient::build_segmented_messages(
        Some("stable instructions"),
        Some("turn memory and hooks"),
        &[system("checkpoint summary"), user("latest request")],
        None,
        &[],
    )
    .unwrap();

    assert_eq!(
        roles(&assembly.messages),
        ["system", "system", "user", "system"]
    );
    assert_eq!(assembly.stable_prefix_messages, 2);
    assert_eq!(
        assembly.messages[1].content.as_str(),
        Some("checkpoint summary")
    );
    assert_eq!(
        assembly.messages[3].content.as_str(),
        Some("turn memory and hooks")
    );
    assert_eq!(
        assembly.segments,
        vec![
            crate::request::PromptSegment {
                lifetime: crate::request::PromptSegmentLifetime::SessionEpoch,
                messages: 0..2,
            },
            crate::request::PromptSegment {
                lifetime: crate::request::PromptSegmentLifetime::AppendOnly,
                messages: 2..3,
            },
            crate::request::PromptSegment {
                lifetime: crate::request::PromptSegmentLifetime::LogicalTurn,
                messages: 3..4,
            },
        ]
    );
    let prepared = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::MainTurn,
        "openai",
        "gpt-5.6-sol",
        assembly.messages.clone(),
        Vec::new(),
        ChatRequestOptions::default(),
    );
    let serialized = crate::agent::provider::openai_responses_bytes_for_test(&prepared).unwrap();
    assert_eq!(
        String::from_utf8(serialized)
            .unwrap()
            .matches("checkpoint summary")
            .count(),
        1,
        "the checkpoint must reach the provider wire exactly once"
    );
}

#[test]
fn segmented_tool_continuation_keeps_the_turn_image_on_its_latest_user_message() {
    let image = crate::image_content::normalize_rgba(1, 1, vec![0, 0, 0, 255]).unwrap();
    let messages = [
        user("inspect this image"),
        assistant_with_calls(vec![call("call-image", ToolStatus::Done, Some("done"))]),
    ];

    let first = AgentClient::build_segmented_messages(
        Some("stable instructions"),
        Some("turn context"),
        &messages,
        None,
        std::slice::from_ref(&image),
    )
    .unwrap();
    let repeated = AgentClient::build_segmented_messages(
        Some("stable instructions"),
        Some("turn context"),
        &messages,
        None,
        &[image],
    )
    .unwrap();

    assert!(first.messages[1].content.has_images());
    assert_eq!(
        serde_json::to_vec(&first.messages).unwrap(),
        serde_json::to_vec(&repeated.messages).unwrap(),
        "a continuation must reproduce the frozen multimodal turn envelope"
    );
}

#[test]
fn prepared_prompt_cache_key_is_stable_and_route_layout_scoped() {
    let scope = crate::request::PromptCacheScope::conversation(
        uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000151").unwrap(),
    );
    let request = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::MainTurn,
        "openai",
        "gpt-5.6-sol",
        vec![ChatMessage {
            role: "system".into(),
            content: crate::image_content::ModelContent::text("stable"),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        }],
        Vec::new(),
        ChatRequestOptions::default(),
    )
    .with_prompt_cache(scope, 1);
    let intent = request.prompt_cache.unwrap();
    let source = crate::agent::stream::ProviderStateSourceV1::from_route(
        "openai",
        crate::agent::ProviderProtocol::OpenAiResponses,
        "https://api.openai.com/v1",
    )
    .unwrap();
    let repeated = prompt_cache_key_for_request(
        intent,
        &source,
        crate::agent::ProviderProtocol::OpenAiResponses,
        "gpt-5.6-sol",
        "openai_direct_v1",
        "openai_responses_v2",
        &request,
    )
    .unwrap();
    let same = prompt_cache_key_for_request(
        intent,
        &source,
        crate::agent::ProviderProtocol::OpenAiResponses,
        "gpt-5.6-sol",
        "openai_direct_v1",
        "openai_responses_v2",
        &request,
    )
    .unwrap();

    let mut continuation = request.clone();
    continuation.kind = crate::request::RequestKind::ToolContinuation;
    continuation.messages.extend([
        ChatMessage {
            role: "assistant".into(),
            content: crate::image_content::ModelContent::text("using a tool"),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
        ChatMessage {
            role: "tool".into(),
            content: crate::image_content::ModelContent::text("tool result"),
            tool_calls: None,
            tool_call_id: Some("call-151".into()),
            response_items: Vec::new(),
        },
    ]);
    let continuation_key = prompt_cache_key_for_request(
        continuation.prompt_cache.unwrap(),
        &source,
        crate::agent::ProviderProtocol::OpenAiResponses,
        "gpt-5.6-sol",
        "openai_direct_v1",
        "openai_responses_v2",
        &continuation,
    )
    .unwrap();
    let other_endpoint = crate::agent::stream::ProviderStateSourceV1::from_route(
        "openai",
        crate::agent::ProviderProtocol::OpenAiResponses,
        "https://proxy.example/v1",
    )
    .unwrap();
    let rotated = prompt_cache_key_for_request(
        intent,
        &other_endpoint,
        crate::agent::ProviderProtocol::OpenAiResponses,
        "gpt-5.6-sol",
        "openai_direct_v1",
        "openai_responses_v2",
        &request,
    )
    .unwrap();

    assert_eq!(repeated, same);
    assert_eq!(
        repeated, continuation_key,
        "a tool continuation must keep the logical turn's cache identity"
    );
    assert_ne!(repeated, rotated);
    assert_eq!(repeated.len(), 64);
    assert!(!repeated.contains("151"));
}

#[test]
fn runtime_prompt_cache_contract_sends_fields_only_for_declared_routes() {
    use crate::config::{
        CaConfig, ModelPromptCacheConfig, PromptCachePolicy, PromptCacheRequestFormat,
        PromptCacheUsageFormat,
    };

    let scope = crate::request::PromptCacheScope::conversation(
        uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000152").unwrap(),
    );
    let request_for = |provider: &str, model: &str| {
        crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::MainTurn,
            provider,
            model,
            vec![
                ChatMessage {
                    role: "system".into(),
                    content: crate::image_content::ModelContent::text("stable epoch"),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: crate::image_content::ModelContent::text("dynamic turn"),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
            ],
            Vec::new(),
            ChatRequestOptions::default(),
        )
        .with_prompt_cache(scope, 1)
    };
    let provider = |base_url: &str, protocol| ProviderConfig {
        google: None,
        tool_schema_policy: Default::default(),
        base_url: base_url.into(),
        api_key: String::new(),
        protocol,
        prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
        models: Vec::new(),
        timeout: ProviderTimeoutConfig::default(),
    };

    let openai = provider(
        "https://api.openai.com/v1",
        crate::agent::ProviderProtocol::OpenAiResponses,
    );
    let openai_request = request_for("openai", "gpt-5.6-sol");
    let openai_client = AgentClient::with_policy(
        "openai",
        &openai,
        "gpt-5.6-sol",
        fast_policy(),
        crate::request::RequestStore::default(),
    );
    let openai_source = crate::agent::stream::ProviderStateSourceV1::from_route(
        "openai",
        openai.protocol,
        &openai.base_url,
    )
    .unwrap();
    let openai_cache = openai_client
        .prompt_cache_for_request(&openai_request, &openai_source)
        .unwrap();
    let openai_body: serde_json::Value = serde_json::from_slice(
        &openai_client
            .protocol
            .adapter()
            .prepare(
                &openai_request,
                &openai_source,
                openai_client.contract,
                openai_client.responses_options.as_ref(),
                openai_cache.as_ref(),
            )
            .unwrap()
            .body,
    )
    .unwrap();
    assert!(openai_body.get("prompt_cache_key").is_some());
    assert!(openai_body.get("prompt_cache_options").is_none());

    for (provider_id, base_url, protocol, model) in [
        (
            "openai-codex",
            "https://chatgpt.com/backend-api/codex",
            crate::agent::ProviderProtocol::OpenAiResponses,
            "gpt-5.6-sol",
        ),
        (
            "mimo",
            "https://api.xiaomimimo.com/v1",
            crate::agent::ProviderProtocol::ChatCompletions,
            "mimo-v2.5-pro",
        ),
        (
            "deepseek",
            "https://api.deepseek.com/v1",
            crate::agent::ProviderProtocol::ChatCompletions,
            "deepseek-v4-pro",
        ),
    ] {
        let provider = provider(base_url, protocol);
        let request = request_for(provider_id, model);
        let client = AgentClient::with_policy(
            provider_id,
            &provider,
            model,
            fast_policy(),
            crate::request::RequestStore::default(),
        );
        let source = crate::agent::stream::ProviderStateSourceV1::from_route(
            provider_id,
            protocol,
            base_url,
        )
        .unwrap();
        assert!(
            client
                .prompt_cache_for_request(&request, &source)
                .unwrap()
                .is_none(),
            "{provider_id} must remain provider-managed"
        );
    }

    let custom = provider(
        "https://compatible.example/v1",
        crate::agent::ProviderProtocol::OpenAiResponses,
    );
    let mut config = CaConfig::default();
    config.providers.insert("custom".into(), custom.clone());
    config.model_prompt_cache.insert(
        "custom/model-x".into(),
        ModelPromptCacheConfig {
            request_format: Some(PromptCacheRequestFormat::OpenAiResponsesExplicit),
            usage_format: Some(PromptCacheUsageFormat::Auto),
            policy: Some(PromptCachePolicy::StablePrefix),
        },
    );
    let custom_contract =
        crate::provider_contract::resolve_prompt_cache(&config, "custom", &custom, "model-x");
    let mut custom_request = request_for("custom", "model-x");
    custom_request.messages.push(ChatMessage {
        role: "system".into(),
        content: crate::image_content::ModelContent::text("turn memory and hooks"),
        tool_calls: None,
        tool_call_id: None,
        response_items: Vec::new(),
    });
    let custom_client = AgentClient::with_policy(
        "custom",
        &custom,
        "model-x",
        fast_policy(),
        crate::request::RequestStore::default(),
    )
    .with_prompt_cache_contract(custom_contract);
    let custom_source = crate::agent::stream::ProviderStateSourceV1::from_route(
        "custom",
        custom.protocol,
        &custom.base_url,
    )
    .unwrap();
    let custom_cache = custom_client
        .prompt_cache_for_request(&custom_request, &custom_source)
        .unwrap();
    let custom_body: serde_json::Value = serde_json::from_slice(
        &custom_client
            .protocol
            .adapter()
            .prepare(
                &custom_request,
                &custom_source,
                custom_client.contract,
                custom_client.responses_options.as_ref(),
                custom_cache.as_ref(),
            )
            .unwrap()
            .body,
    )
    .unwrap();
    assert!(custom_body.get("prompt_cache_key").is_some());
    assert_eq!(custom_body["prompt_cache_options"]["mode"], "explicit");
    assert_eq!(
        custom_body["input"][0]["content"][0]["prompt_cache_breakpoint"]["mode"],
        "explicit"
    );
    assert!(custom_body["input"][2]
        .to_string()
        .find("prompt_cache_breakpoint")
        .is_none());
}

#[test]
fn prompt_cache_epoch_is_an_extension_or_has_a_recognized_invalidation() {
    let session_id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000153").unwrap();
    let scope = crate::request::PromptCacheScope::conversation(session_id);
    let source = crate::agent::stream::ProviderStateSourceV1::from_route(
        "openai",
        crate::agent::ProviderProtocol::OpenAiResponses,
        "https://api.openai.com/v1",
    )
    .unwrap();
    let tools = vec![ToolDef {
        def_type: "function".into(),
        function: ToolFunction {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
            }),
        },
    }];
    let base = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::MainTurn,
        "openai",
        "gpt-5.6-sol",
        vec![
            ChatMessage {
                role: "system".into(),
                content: crate::image_content::ModelContent::text("session epoch"),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            },
            ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text("logical request"),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            },
        ],
        tools,
        ChatRequestOptions::default(),
    )
    .with_prompt_cache(scope, 1);
    let key_for = |intent,
                   source: &crate::agent::stream::ProviderStateSourceV1,
                   protocol,
                   model,
                   contract,
                   layout,
                   request: &crate::request::PreparedModelRequest| {
        prompt_cache_key_for_request(intent, source, protocol, model, contract, layout, request)
            .unwrap()
    };
    let base_key = key_for(
        base.prompt_cache.unwrap(),
        &source,
        crate::agent::ProviderProtocol::OpenAiResponses,
        "gpt-5.6-sol",
        "openai_direct_v1",
        "openai_responses_v2",
        &base,
    );

    let mut exact_extension = base.clone();
    exact_extension.kind = crate::request::RequestKind::ToolContinuation;
    exact_extension.messages.push(ChatMessage {
        role: "assistant".into(),
        content: crate::image_content::ModelContent::text("append-only continuation"),
        tool_calls: None,
        tool_call_id: None,
        response_items: Vec::new(),
    });
    for (lifecycle, request) in [
        ("transport retry", &base),
        ("saved-session resume", &base),
        ("tool continuation", &exact_extension),
    ] {
        assert_eq!(
            base_key,
            key_for(
                request.prompt_cache.unwrap(),
                &source,
                crate::agent::ProviderProtocol::OpenAiResponses,
                "gpt-5.6-sol",
                "openai_direct_v1",
                "openai_responses_v2",
                request,
            ),
            "{lifecycle} must extend the same cache epoch"
        );
    }

    let mut branch = base.clone();
    branch.prompt_cache = Some(crate::request::PromptCacheIntent {
        scope: crate::request::PromptCacheScope::conversation(
            uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000154").unwrap(),
        ),
        stable_prefix_messages: 1,
    });
    let mut side_question = base.clone();
    side_question.prompt_cache = Some(crate::request::PromptCacheIntent {
        scope: crate::request::PromptCacheScope::side_question(session_id),
        stable_prefix_messages: 1,
    });
    let mut subagent = base.clone();
    subagent.prompt_cache = Some(crate::request::PromptCacheIntent {
        scope: crate::request::PromptCacheScope::agent(session_id),
        stable_prefix_messages: 1,
    });
    let mut changed_prefix = base.clone();
    changed_prefix.messages[0].content =
        crate::image_content::ModelContent::text("changed session epoch");
    let mut changed_tools = base.clone();
    changed_tools.tools[0].function.parameters = serde_json::json!({
        "type": "object",
        "properties": {"path": {"type": "string"}, "line": {"type": "integer"}},
    });
    let proxy = crate::agent::stream::ProviderStateSourceV1::from_route(
        "openai",
        crate::agent::ProviderProtocol::OpenAiResponses,
        "https://proxy.example/v1",
    )
    .unwrap();
    let fallback = crate::agent::stream::ProviderStateSourceV1::from_route(
        "fallback",
        crate::agent::ProviderProtocol::OpenAiResponses,
        "https://api.openai.com/v1",
    )
    .unwrap();
    let invalidations = [
        (
            "branch scope",
            key_for(
                branch.prompt_cache.unwrap(),
                &source,
                crate::agent::ProviderProtocol::OpenAiResponses,
                "gpt-5.6-sol",
                "openai_direct_v1",
                "openai_responses_v2",
                &branch,
            ),
        ),
        (
            "side-question domain",
            key_for(
                side_question.prompt_cache.unwrap(),
                &source,
                crate::agent::ProviderProtocol::OpenAiResponses,
                "gpt-5.6-sol",
                "openai_direct_v1",
                "openai_responses_v2",
                &side_question,
            ),
        ),
        (
            "subagent domain",
            key_for(
                subagent.prompt_cache.unwrap(),
                &source,
                crate::agent::ProviderProtocol::OpenAiResponses,
                "gpt-5.6-sol",
                "openai_direct_v1",
                "openai_responses_v2",
                &subagent,
            ),
        ),
        (
            "endpoint",
            key_for(
                base.prompt_cache.unwrap(),
                &proxy,
                crate::agent::ProviderProtocol::OpenAiResponses,
                "gpt-5.6-sol",
                "openai_direct_v1",
                "openai_responses_v2",
                &base,
            ),
        ),
        (
            "fallback provider",
            key_for(
                base.prompt_cache.unwrap(),
                &fallback,
                crate::agent::ProviderProtocol::OpenAiResponses,
                "gpt-5.6-sol",
                "openai_direct_v1",
                "openai_responses_v2",
                &base,
            ),
        ),
        (
            "protocol",
            key_for(
                base.prompt_cache.unwrap(),
                &source,
                crate::agent::ProviderProtocol::ChatCompletions,
                "gpt-5.6-sol",
                "openai_direct_v1",
                "openai_responses_v2",
                &base,
            ),
        ),
        (
            "model or one-turn route",
            key_for(
                base.prompt_cache.unwrap(),
                &source,
                crate::agent::ProviderProtocol::OpenAiResponses,
                "gpt-5.6-terra",
                "openai_direct_v1",
                "openai_responses_v2",
                &base,
            ),
        ),
        (
            "contract version",
            key_for(
                base.prompt_cache.unwrap(),
                &source,
                crate::agent::ProviderProtocol::OpenAiResponses,
                "gpt-5.6-sol",
                "openai_direct_v2",
                "openai_responses_v2",
                &base,
            ),
        ),
        (
            "request layout version",
            key_for(
                base.prompt_cache.unwrap(),
                &source,
                crate::agent::ProviderProtocol::OpenAiResponses,
                "gpt-5.6-sol",
                "openai_direct_v1",
                "openai_responses_v3",
                &base,
            ),
        ),
        (
            "session epoch digest",
            key_for(
                changed_prefix.prompt_cache.unwrap(),
                &source,
                crate::agent::ProviderProtocol::OpenAiResponses,
                "gpt-5.6-sol",
                "openai_direct_v1",
                "openai_responses_v2",
                &changed_prefix,
            ),
        ),
        (
            "canonical tools digest",
            key_for(
                changed_tools.prompt_cache.unwrap(),
                &source,
                crate::agent::ProviderProtocol::OpenAiResponses,
                "gpt-5.6-sol",
                "openai_direct_v1",
                "openai_responses_v2",
                &changed_tools,
            ),
        ),
    ];
    for (recognized_reason, key) in invalidations {
        assert_ne!(
            base_key, key,
            "cache rotation must record recognized invalidation: {recognized_reason}"
        );
    }
}

#[test]
fn reasoning_request_fields_are_explicit_and_absent_without_a_complete_profile() {
    let absent = request_json_for_test(&ChatRequestOptions {
        reasoning_effort: Some(ReasoningEffort::High),
        ..ChatRequestOptions::default()
    });
    assert!(absent.get("reasoning_effort").is_none());
    assert!(absent.get("reasoning").is_none());

    let flat = request_json_for_test(&ChatRequestOptions {
        reasoning_effort: Some(ReasoningEffort::Low),
        reasoning_request_format: Some(ReasoningRequestFormat::ReasoningEffort),
        reasoning_stream_field: Some("reasoning_content".into()),
        tool_call_format: None,
    });
    assert_eq!(flat["reasoning_effort"], "low");
    assert!(flat.get("reasoning").is_none());

    let nested = request_json_for_test(&ChatRequestOptions {
        reasoning_effort: Some(ReasoningEffort::Max),
        reasoning_request_format: Some(ReasoningRequestFormat::ReasoningObject),
        reasoning_stream_field: Some("analysis".into()),
        tool_call_format: None,
    });
    assert_eq!(nested["reasoning"]["effort"], "max");
    assert!(nested.get("reasoning_effort").is_none());
}

#[test]
fn prepared_request_keeps_the_existing_chat_completions_json_shape() {
    let messages = vec![ChatMessage {
        role: "user".into(),
        content: crate::image_content::ModelContent::text("hello"),
        tool_calls: None,
        tool_call_id: None,
        response_items: Vec::new(),
    }];
    let tools = vec![ToolDef {
        def_type: "function".into(),
        function: ToolFunction {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: serde_json::json!({"type": "object"}),
        },
    }];
    let prepared = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::MainTurn,
        "local",
        "model",
        messages.clone(),
        tools.clone(),
        ChatRequestOptions {
            reasoning_effort: Some(ReasoningEffort::High),
            reasoning_request_format: Some(ReasoningRequestFormat::ReasoningObject),
            reasoning_stream_field: Some("analysis".into()),
            tool_call_format: None,
        },
    );
    let actual = crate::agent::provider::chat_completions_json_for_test(&prepared);
    assert_eq!(
        actual,
        serde_json::json!({
            "model": "model",
            "messages": messages,
            "tools": tools,
            "stream": true,
            "reasoning": {"effort": "high"}
        })
    );

    let empty_tools = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::ProviderProbe,
        "local",
        "model",
        Vec::new(),
        Vec::new(),
        ChatRequestOptions::default(),
    );
    assert!(
        crate::agent::provider::chat_completions_json_for_test(&empty_tools)
            .get("tools")
            .is_none()
    );
}

#[test]
fn prepared_request_keeps_the_exact_chat_completions_json_bytes() {
    let prepared = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::MainTurn,
        "local",
        "model",
        vec![ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text("hello"),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        }],
        vec![ToolDef {
            def_type: "function".into(),
            function: ToolFunction {
                name: "read_file".into(),
                description: "Read a file".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
        }],
        ChatRequestOptions {
            reasoning_effort: Some(ReasoningEffort::High),
            reasoning_request_format: Some(ReasoningRequestFormat::ReasoningObject),
            reasoning_stream_field: Some("analysis".into()),
            tool_call_format: None,
        },
    );

    let actual = crate::agent::provider::chat_completions_bytes_for_test(&prepared);

    assert_eq!(
        actual,
        br#"{"model":"model","messages":[{"role":"user","content":"hello"}],"tools":[{"type":"function","function":{"name":"read_file","description":"Read a file","parameters":{"type":"object"}}}],"stream":true,"reasoning":{"effort":"high"}}"#
    );
}

#[test]
fn completed_tool_call_serializes_after_assistant() {
    let state_messages = vec![
        user("read input.txt"),
        assistant_with_calls(vec![call("call-1", ToolStatus::Done, Some("hello"))]),
    ];

    let provider = AgentClient::build_messages(None, &state_messages, None, &[]).unwrap();

    assert_eq!(roles(&provider), vec!["user", "assistant", "tool"]);
    assert_eq!(provider[1].tool_calls.as_ref().unwrap()[0].id, "call-1");
    assert_eq!(provider[2].tool_call_id.as_deref(), Some("call-1"));
    assert_eq!(provider[2].content.as_str(), Some("hello"));
}

#[test]
fn completed_tool_continuation_keeps_exact_adapter_bytes() {
    let provider_messages = AgentClient::build_messages(
        None,
        &[assistant_with_calls(vec![call(
            "call-1",
            ToolStatus::Done,
            Some("hello"),
        )])],
        None,
        &[],
    )
    .unwrap();
    let prepared = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::ToolContinuation,
        "local",
        "model",
        provider_messages,
        Vec::new(),
        ChatRequestOptions::default(),
    );

    assert_eq!(
        crate::agent::provider::chat_completions_bytes_for_test(&prepared),
        br#"{"model":"model","messages":[{"role":"assistant","content":"","tool_calls":[{"id":"call-1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"input.txt\"}"}}]},{"role":"tool","content":"hello","tool_call_id":"call-1"}],"stream":true}"#
    );
}

#[test]
fn reasoning_content_tool_continuation_is_provider_scoped_and_byte_faithful() {
    let exact_reasoning = "  inspect\nλ\\tail  ";
    let mut assistant = assistant_with_calls(vec![call("call-1", ToolStatus::Done, Some("hello"))]);
    assistant.thinking = Some(exact_reasoning.into());
    assistant.response_items = vec![crate::agent::stream::ProviderResponseItem::ChatReasoning {
        source: crate::agent::stream::ProviderStateSourceV1::for_test(
            "deepseek",
            crate::agent::provider::ProviderProtocol::ChatCompletions,
        ),
        content: exact_reasoning.into(),
    }];
    let provider_messages = AgentClient::build_messages(None, &[assistant], None, &[]).unwrap();

    for (provider, model, expected) in [
        ("deepseek", "deepseek-v4-pro", Some(exact_reasoning)),
        ("mimo", "mimo-v2.5-pro", None),
    ] {
        let prepared = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::ToolContinuation,
            provider,
            model,
            provider_messages.clone(),
            Vec::new(),
            ChatRequestOptions::default(),
        );
        let value = crate::agent::provider::chat_completions_json_for_test(&prepared);
        assert_eq!(
            value["messages"][0]
                .get("reasoning_content")
                .and_then(serde_json::Value::as_str),
            expected,
            "reasoning_content must not cross provider boundaries"
        );
    }

    let ordinary = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::ToolContinuation,
        "openrouter",
        "model",
        provider_messages,
        Vec::new(),
        ChatRequestOptions::default(),
    );
    assert!(
        crate::agent::provider::chat_completions_json_for_test(&ordinary)["messages"][0]
            .get("reasoning_content")
            .is_none()
    );

    let mut unprovenanced =
        assistant_with_calls(vec![call("call-2", ToolStatus::Done, Some("hello"))]);
    unprovenanced.thinking = Some("foreign visible thinking".into());
    let unprovenanced = AgentClient::build_messages(None, &[unprovenanced], None, &[]).unwrap();
    let deepseek = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::ToolContinuation,
        "deepseek",
        "deepseek-v4-pro",
        unprovenanced,
        Vec::new(),
        ChatRequestOptions::default(),
    );
    assert!(
        crate::agent::provider::chat_completions_json_for_test(&deepseek)["messages"][0]
            .get("reasoning_content")
            .is_none()
    );
}

#[test]
fn final_reasoning_replay_is_deepseek_only_and_requires_provenance() {
    for (provider, model, expected) in [
        ("deepseek", "deepseek-v4-pro", Some("  final\n雪  ")),
        ("mimo", "mimo-v2.5-pro", None),
    ] {
        let mut assistant = Message {
            role: Role::Assistant,
            content: "answer".into(),
            tool_calls: None,
            thinking: Some("display-only reasoning".into()),
            response_items: vec![ProviderResponseItem::ChatReasoning {
                source: crate::agent::stream::ProviderStateSourceV1::for_test(
                    provider,
                    crate::agent::ProviderProtocol::ChatCompletions,
                ),
                content: "  final\n雪  ".into(),
            }],
            timestamp: fixed_time(),
        };
        for provenanced in [true, false] {
            if !provenanced {
                assistant.response_items.clear();
            }
            let request = crate::request::PreparedModelRequest::new(
                crate::request::RequestKind::MainTurn,
                provider,
                model,
                AgentClient::build_messages(None, &[assistant.clone()], None, &[]).unwrap(),
                Vec::new(),
                ChatRequestOptions::default(),
            );
            let value = crate::agent::provider::chat_completions_json_for_test(&request);
            assert_eq!(
                value["messages"][0]
                    .get("reasoning_content")
                    .and_then(Value::as_str),
                if provenanced { expected } else { None },
            );
        }
    }
}

#[test]
fn multiple_tool_results_preserve_call_order() {
    let state_messages = vec![assistant_with_calls(vec![
        call("call-a", ToolStatus::Done, Some("alpha")),
        call("call-b", ToolStatus::Done, Some("beta")),
    ])];

    let provider = AgentClient::build_messages(None, &state_messages, None, &[]).unwrap();

    assert_eq!(roles(&provider), vec!["assistant", "tool", "tool"]);
    assert_eq!(
        provider
            .iter()
            .filter_map(|message| message.tool_call_id.as_deref())
            .collect::<Vec<_>>(),
        vec!["call-a", "call-b"]
    );
    assert_eq!(provider[1].content.as_str(), Some("alpha"));
    assert_eq!(provider[2].content.as_str(), Some("beta"));
}

#[test]
fn denied_tool_is_serialized_as_tool_result() {
    let state_messages = vec![assistant_with_calls(vec![call(
        "call-denied",
        ToolStatus::Error,
        Some("Denied"),
    )])];

    let provider = AgentClient::build_messages(None, &state_messages, None, &[]).unwrap();

    assert_eq!(roles(&provider), vec!["assistant", "tool"]);
    assert_eq!(provider[1].tool_call_id.as_deref(), Some("call-denied"));
    assert_eq!(provider[1].content.as_str(), Some("Denied"));
}

#[test]
fn tool_execution_error_is_serialized_as_tool_result() {
    let state_messages = vec![assistant_with_calls(vec![call(
        "call-error",
        ToolStatus::Error,
        Some("Error: input.txt not found"),
    )])];

    let provider = AgentClient::build_messages(None, &state_messages, None, &[]).unwrap();

    assert_eq!(roles(&provider), vec!["assistant", "tool"]);
    assert_eq!(provider[1].tool_call_id.as_deref(), Some("call-error"));
    assert_eq!(
        provider[1].content.as_str(),
        Some("Error: input.txt not found")
    );
}

#[test]
fn empty_tool_output_is_still_a_completed_result() {
    let state_messages = vec![assistant_with_calls(vec![call(
        "call-empty",
        ToolStatus::Done,
        Some(""),
    )])];

    let provider = AgentClient::build_messages(None, &state_messages, None, &[]).unwrap();

    assert_eq!(roles(&provider), vec!["assistant", "tool"]);
    assert_eq!(provider[1].tool_call_id.as_deref(), Some("call-empty"));
    assert_eq!(provider[1].content.as_str(), Some(""));
}

#[test]
fn unresolved_tool_call_prevents_request_construction() {
    let state_messages = vec![assistant_with_calls(vec![call(
        "call-pending",
        ToolStatus::Running,
        None,
    )])];

    let error = AgentClient::build_messages(None, &state_messages, None, &[])
        .expect_err("unresolved calls must not reach the provider");

    assert!(error.to_string().contains("call-pending"));
    assert!(error.to_string().contains("no result"));
}

#[test]
fn unresolved_incremental_append_does_not_partially_mutate_request() {
    let mut provider = vec![ChatMessage {
        role: "user".into(),
        content: crate::image_content::ModelContent::text("keep me"),
        tool_calls: None,
        tool_call_id: None,
        response_items: Vec::new(),
    }];
    let pending = assistant_with_calls(vec![call("call-pending", ToolStatus::Running, None)]);

    let error = append_provider_messages(&mut provider, &pending)
        .expect_err("unresolved calls must fail before mutating the request");

    assert!(error.to_string().contains("call-pending"));
    assert_eq!(roles(&provider), vec!["user"]);
    assert_eq!(provider[0].content.as_str(), Some("keep me"));
}

#[test]
fn duplicate_tool_result_id_prevents_request_construction() {
    let state_messages = vec![
        assistant_with_calls(vec![call("call-dup", ToolStatus::Done, Some("first"))]),
        assistant_with_calls(vec![call("call-dup", ToolStatus::Done, Some("second"))]),
    ];

    let error = AgentClient::build_messages(None, &state_messages, None, &[])
        .expect_err("duplicate tool results must not be emitted");

    assert!(error.to_string().contains("call-dup"));
    assert!(error.to_string().contains("duplicate"));
}

#[test]
fn legacy_tool_message_still_serializes_once() {
    let legacy = Message {
        role: Role::Tool,
        content: String::new(),
        tool_calls: Some(vec![call("legacy-call", ToolStatus::Done, Some("legacy"))]),
        thinking: None,
        response_items: Vec::new(),
        timestamp: fixed_time(),
    };

    let provider = AgentClient::build_messages(None, &[legacy], None, &[]).unwrap();

    assert_eq!(roles(&provider), vec!["tool"]);
    assert_eq!(provider[0].tool_call_id.as_deref(), Some("legacy-call"));
}

#[test]
fn restored_completed_assistant_turn_serializes_identically() {
    let original = vec![assistant_with_calls(vec![call(
        "restored-call",
        ToolStatus::Done,
        Some("restored result"),
    )])];
    let encoded = serde_json::to_string(&original).unwrap();
    let restored: Vec<Message> = serde_json::from_str(&encoded).unwrap();

    let before = AgentClient::build_messages(None, &original, None, &[]).unwrap();
    let after = AgentClient::build_messages(None, &restored, None, &[]).unwrap();

    assert_eq!(
        serde_json::to_value(&before).unwrap(),
        serde_json::to_value(&after).unwrap()
    );
    assert_eq!(roles(&after), vec!["assistant", "tool"]);
    assert_eq!(after[1].tool_call_id.as_deref(), Some("restored-call"));
}

#[test]
fn mixed_assistant_and_legacy_duplicate_is_rejected() {
    let assistant = assistant_with_calls(vec![call(
        "call-mixed",
        ToolStatus::Done,
        Some("assistant result"),
    )]);
    let legacy = Message {
        role: Role::Tool,
        content: String::new(),
        tool_calls: Some(vec![call(
            "call-mixed",
            ToolStatus::Done,
            Some("legacy result"),
        )]),
        thinking: None,
        response_items: Vec::new(),
        timestamp: fixed_time(),
    };

    let error = AgentClient::build_messages(None, &[assistant, legacy], None, &[])
        .expect_err("mixed duplicate results must be rejected");

    assert!(error.to_string().contains("call-mixed"));
    assert!(error.to_string().contains("duplicate"));
}

#[test]
fn runtime_client_debug_redacts_endpoint_and_credential() {
    let client = AgentClient::new(
        &ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: "https://user:password@example.invalid/v1?token=query-secret".into(),
            api_key: "sentinel-api-key".into(),
            protocol: crate::agent::ProviderProtocol::default(),
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["model".into()],
            timeout: ProviderTimeoutConfig::default(),
        },
        "model",
    );
    let debug = format!("{client:?}");

    assert!(debug.contains("model"));
    assert!(debug.contains("[REDACTED]"));
    assert!(!debug.contains("sentinel-api-key"));
    assert!(!debug.contains("password"));
    assert!(!debug.contains("query-secret"));
}

fn fast_policy() -> ProviderCallPolicy {
    ProviderCallPolicy {
        connect_timeout: std::time::Duration::from_millis(50),
        headers_timeout: std::time::Duration::from_millis(50),
        first_event_timeout: std::time::Duration::from_millis(40),
        no_progress_timeout: std::time::Duration::from_millis(40),
        max_attempts: 1,
        retry_delay: std::time::Duration::from_millis(1),
        max_retry_after: std::time::Duration::from_millis(5),
    }
}

#[test]
fn one_attempt_policy_preserves_default_deadlines_and_disables_replay() {
    let default = ProviderCallPolicy::default();
    let one_attempt = ProviderCallPolicy::one_attempt();

    assert_eq!(default.max_attempts, 2);
    assert_eq!(default.headers_timeout, std::time::Duration::from_secs(60));
    assert_eq!(
        default.first_event_timeout,
        std::time::Duration::from_secs(60)
    );
    assert_eq!(
        default.no_progress_timeout,
        std::time::Duration::from_secs(120)
    );
    assert_eq!(one_attempt.max_attempts, 1);
    assert_eq!(one_attempt.connect_timeout, default.connect_timeout);
    assert_eq!(one_attempt.headers_timeout, default.headers_timeout);
    assert_eq!(one_attempt.first_event_timeout, default.first_event_timeout);
    assert_eq!(one_attempt.no_progress_timeout, default.no_progress_timeout);
    assert_eq!(one_attempt.retry_delay, default.retry_delay);
    assert_eq!(one_attempt.max_retry_after, default.max_retry_after);
}

#[test]
fn transport_regression_one_attempt_preserves_custom_policy() {
    let mut policy = fast_policy();
    policy.max_attempts = 2;
    let client =
        test_client("http://127.0.0.1:1/v1".into(), policy.clone()).with_one_attempt_policy();
    assert_eq!(client.policy.max_attempts, 1);
    assert_eq!(client.policy.connect_timeout, policy.connect_timeout);
    assert_eq!(client.policy.headers_timeout, policy.headers_timeout);
    assert_eq!(
        client.policy.first_event_timeout,
        policy.first_event_timeout
    );
    assert_eq!(
        client.policy.no_progress_timeout,
        policy.no_progress_timeout
    );
    assert_eq!(client.policy.retry_delay, policy.retry_delay);
    assert_eq!(client.policy.max_retry_after, policy.max_retry_after);
}

#[tokio::test]
async fn transport_regression_one_attempt_preserves_http_client() {
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        request
    });
    let mut client = test_client(url, fast_policy());
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("x-transport-sentinel", "preserved".parse().unwrap());
    client.client = Client::builder().default_headers(headers).build().unwrap();
    let error = client
        .with_one_attempt_policy()
        .chat_stream(&[], &[])
        .await
        .unwrap_err();
    assert_eq!(error, ProviderError::http(400));
    let request = String::from_utf8(server.await.unwrap()).unwrap();
    assert!(
        request.contains("x-transport-sentinel: preserved"),
        "existing HTTP client was replaced"
    );
}

#[test]
fn responses_only_options_and_client_request_id_fail_closed() {
    fn client(protocol: crate::agent::ProviderProtocol, policy: ProviderCallPolicy) -> AgentClient {
        AgentClient::with_call_policy(
            "test",
            &ProviderConfig {
                google: None,
                tool_schema_policy: Default::default(),
                base_url: "http://127.0.0.1:1/v1".into(),
                api_key: String::new(),
                protocol,
                prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                models: vec!["model".into()],
                timeout: ProviderTimeoutConfig::default(),
            },
            "model",
            policy,
            crate::request::RequestStore::default(),
        )
    }

    let capped = OpenAiResponsesRequestOptions::capped(Some(OpenAiServiceTier::Default));
    assert!(client(
        crate::agent::ProviderProtocol::ChatCompletions,
        ProviderCallPolicy::one_attempt()
    )
    .with_openai_responses_options(capped)
    .is_err());
    assert!(client(
        crate::agent::ProviderProtocol::ChatCompletions,
        ProviderCallPolicy::one_attempt()
    )
    .with_openai_client_request_id("ledger-1")
    .is_err());
    assert!(client(
        crate::agent::ProviderProtocol::OpenAiResponses,
        ProviderCallPolicy::default()
    )
    .with_openai_client_request_id("ledger-1")
    .is_err());
    for invalid in [
        String::new(),
        "x".repeat(513),
        "bad\nvalue".into(),
        "λ".into(),
    ] {
        assert!(client(
            crate::agent::ProviderProtocol::OpenAiResponses,
            ProviderCallPolicy::one_attempt()
        )
        .with_openai_client_request_id(invalid)
        .is_err());
    }
}

fn test_client(base_url: String, policy: ProviderCallPolicy) -> AgentClient {
    test_client_with_protocol(
        base_url,
        policy,
        crate::agent::ProviderProtocol::ChatCompletions,
    )
}

fn test_client_with_protocol(
    base_url: String,
    policy: ProviderCallPolicy,
    protocol: crate::agent::ProviderProtocol,
) -> AgentClient {
    AgentClient::with_policy(
        "test",
        &ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url,
            api_key: String::new(),
            protocol,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["model".into()],
            timeout: ProviderTimeoutConfig::default(),
        },
        "model",
        policy,
        crate::request::RequestStore::default(),
    )
}

async fn read_request(stream: &mut tokio::net::TcpStream) {
    use tokio::io::AsyncReadExt;
    let mut request = vec![0_u8; 16 * 1024];
    let _ = stream.read(&mut request).await.unwrap();
}

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
    use tokio::io::AsyncReadExt;

    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = stream.read(&mut chunk).await.unwrap();
        if read == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..read]);
        let Some(header_end) = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|index| index + 4)
        else {
            continue;
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        if request.len() >= header_end + content_length {
            break;
        }
    }
    request
}

fn request_body(request: &[u8]) -> &[u8] {
    let header_end = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("captured HTTP request has headers")
        + 4;
    &request[header_end..]
}

async fn capture_reasoning_content_tool_loop(provider: &'static str) -> Vec<Vec<u8>> {
    use tokio::io::AsyncWriteExt;

    let exact_reasoning = "  inspect\nλ\\tail  ";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for body in [
            format!(
                "data: {{\"choices\":[{{\"delta\":{{\"reasoning_content\":{},\"tool_calls\":[{{\"index\":0,\"id\":\"call-1\",\"function\":{{\"name\":\"read_file\",\"arguments\":\"{{\\\"path\\\":\\\"input.txt\\\"}}\"}}}}]}},\"finish_reason\":\"tool_calls\"}}]}}\n\ndata: [DONE]\n\n",
                serde_json::to_string(exact_reasoning).unwrap()
            ),
            "data: {\"choices\":[{\"delta\":{\"content\":\"done\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".into(),
        ] {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(read_http_request(&mut stream).await);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(body.as_bytes()).await.unwrap();
        }
        requests
    });
    let model = if provider == "deepseek" {
        "deepseek-v4-pro"
    } else {
        "mimo-v2.5-pro"
    };
    let client = AgentClient::with_policy(
        provider,
        &ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: url,
            api_key: String::new(),
            protocol: crate::agent::ProviderProtocol::ChatCompletions,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec![model.into()],
            timeout: ProviderTimeoutConfig::default(),
        },
        model,
        fast_policy(),
        crate::request::RequestStore::default(),
    )
    .with_contract_for_test(official_contract(
        provider,
        crate::agent::ProviderProtocol::ChatCompletions,
        model,
    ));
    let first_messages = vec![ChatMessage {
        role: "user".into(),
        content: crate::image_content::ModelContent::text("read input.txt"),
        tool_calls: None,
        tool_call_id: None,
        response_items: Vec::new(),
    }];
    let first = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::MainTurn,
        provider,
        model,
        first_messages,
        Vec::new(),
        ChatRequestOptions::default(),
    );
    let mut first_stream = client.stream_prepared(&first).await.unwrap();
    let first_turn = crate::agent::stream::consume_stream(&mut first_stream, |_| {})
        .await
        .unwrap();
    assert_eq!(first_turn.reasoning.as_deref(), Some(exact_reasoning));

    let continuation_state = vec![
        user("read input.txt"),
        Message {
            role: Role::Assistant,
            content: first_turn.content,
            tool_calls: Some(vec![ToolCall {
                id: first_turn.tool_calls[0].id.clone(),
                name: first_turn.tool_calls[0].name.clone(),
                arguments: first_turn.tool_calls[0].arguments.clone(),
                status: ToolStatus::Done,
                output: Some("hello".into()),
            }]),
            thinking: first_turn.reasoning,
            response_items: first_turn.response_items,
            timestamp: fixed_time(),
        },
    ];
    let continuation_messages =
        AgentClient::build_messages(None, &continuation_state, None, &[]).unwrap();
    let continuation = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::ToolContinuation,
        provider,
        model,
        continuation_messages,
        Vec::new(),
        ChatRequestOptions::default(),
    );
    let mut second_stream = client.stream_prepared(&continuation).await.unwrap();
    let second_turn = crate::agent::stream::consume_stream(&mut second_stream, |_| {})
        .await
        .unwrap();
    assert_eq!(second_turn.content, "done");

    server.await.unwrap()
}

#[tokio::test]
async fn deepseek_and_mimo_two_step_loopbacks_replay_exact_reasoning_content() {
    let exact_reasoning = "  inspect\nλ\\tail  ";
    for provider in ["deepseek", "mimo"] {
        let requests = capture_reasoning_content_tool_loop(provider).await;
        assert_eq!(requests.len(), 2);
        let continuation: serde_json::Value =
            serde_json::from_slice(request_body(&requests[1])).unwrap();
        assert_eq!(
            continuation["messages"][1]["reasoning_content"],
            exact_reasoning
        );
        assert_eq!(continuation["messages"][1]["tool_calls"][0]["id"], "call-1");
        assert_eq!(continuation["messages"][2]["tool_call_id"], "call-1");
    }
}

#[tokio::test]
async fn deepseek_replays_tool_and_final_reasoning_on_the_next_question() {
    use tokio::io::AsyncWriteExt;

    let tool_reasoning = "  inspect\nλ\\tool  ";
    let final_reasoning = "  explain\n雪\\final  ";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        let bodies = [
            serde_json::json!({"choices": [{"delta": {
                "reasoning_content": tool_reasoning,
                "tool_calls": [{"index": 0, "id": "call-1", "function": {
                    "name": "read_file", "arguments": r#"{"path":"input.txt"}"#
                }}]
            }, "finish_reason": "tool_calls"}]}),
            serde_json::json!({"choices": [{"delta": {
                "reasoning_content": final_reasoning, "content": "first answer"
            }, "finish_reason": "stop"}]}),
            serde_json::json!({"choices": [{"delta": {"content": "next answer"},
                "finish_reason": "stop"}]}),
        ];
        for body in bodies {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(read_http_request(&mut stream).await);
            let body = format!("data: {body}\n\ndata: [DONE]\n\n");
            stream
                .write_all(
                    format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(body.as_bytes()).await.unwrap();
        }
        requests
    });
    let client = AgentClient::with_policy(
        "deepseek",
        &ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: endpoint.clone(),
            api_key: String::new(),
            protocol: crate::agent::ProviderProtocol::ChatCompletions,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["deepseek-v4-pro".into()],
            timeout: ProviderTimeoutConfig::default(),
        },
        "deepseek-v4-pro",
        fast_policy(),
        crate::request::RequestStore::default(),
    )
    .with_contract_for_test(official_contract(
        "deepseek",
        crate::agent::ProviderProtocol::ChatCompletions,
        "deepseek-v4-pro",
    ));
    let tools = vec![ToolDef {
        def_type: "function".into(),
        function: ToolFunction {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: serde_json::json!({"type": "object"}),
        },
    }];
    let mut history = vec![user("read input.txt")];
    let mut last_request = None;
    for index in 0..3 {
        let request = crate::request::PreparedModelRequest::new(
            if index == 1 {
                crate::request::RequestKind::ToolContinuation
            } else {
                crate::request::RequestKind::MainTurn
            },
            "deepseek",
            "deepseek-v4-pro",
            AgentClient::build_messages(None, &history, None, &[]).unwrap(),
            tools.clone(),
            ChatRequestOptions::default(),
        );
        let mut stream = client.stream_prepared(&request).await.unwrap();
        let turn = crate::agent::stream::consume_stream(&mut stream, |_| {})
            .await
            .unwrap();
        let calls = turn
            .tool_calls
            .into_iter()
            .map(|call| ToolCall {
                id: call.id,
                name: call.name,
                arguments: call.arguments,
                status: ToolStatus::Done,
                output: Some("hello".into()),
            })
            .collect::<Vec<_>>();
        history.push(Message {
            role: Role::Assistant,
            content: turn.content,
            tool_calls: (!calls.is_empty()).then_some(calls),
            thinking: turn.reasoning,
            response_items: turn.response_items,
            timestamp: fixed_time(),
        });
        if index == 1 {
            history.push(user("explain another file"));
        }
        last_request = Some(request);
    }
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 3);
    let next: serde_json::Value = serde_json::from_slice(request_body(&requests[2])).unwrap();
    assert_eq!(next["tools"][0]["function"]["name"], "read_file");
    assert_eq!(next["messages"][1]["reasoning_content"], tool_reasoning);
    assert_eq!(next["messages"][3]["reasoning_content"], final_reasoning);

    let request = last_request.unwrap();
    for provider in ["deepseek", "mimo", "custom"] {
        let source = crate::agent::stream::ProviderStateSourceV1::from_route(
            provider,
            crate::agent::ProviderProtocol::ChatCompletions,
            if provider == "deepseek" {
                "https://other.invalid/v1"
            } else {
                &endpoint
            },
        )
        .unwrap();
        let contract = match provider {
            "deepseek" => Some(official_contract(
                provider,
                crate::agent::ProviderProtocol::ChatCompletions,
                "deepseek-v4-pro",
            )),
            "mimo" => Some(official_contract(
                provider,
                crate::agent::ProviderProtocol::ChatCompletions,
                "mimo-v2.5-pro",
            )),
            _ => None,
        };
        let encoded = crate::agent::ProviderProtocol::ChatCompletions
            .adapter()
            .prepare(&request, &source, contract, None, None)
            .unwrap()
            .body;
        let changed: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        for index in [1, 3] {
            assert!(changed["messages"][index]
                .get("reasoning_content")
                .is_none());
        }
    }
}

#[test]
fn provider_error_excerpt_is_utf8_bounded_and_redacts_complete_credentials() {
    let body = serde_json::json!({"error": {"message": format!("{}雪", "x".repeat(511))}});
    let excerpt = excerpt_from_error_bytes(&serde_json::to_vec(&body).unwrap(), &[]).unwrap();
    assert_eq!(excerpt, "x".repeat(511));
    assert!(excerpt.len() <= ERROR_BODY_EXCERPT_BYTES);

    let body = serde_json::json!({"error": {"message":
        "bad \u{1b}[31mfield\u{1b}[0m Bearer unknown-token\napi_key=unknown-key\nsk-unknown-secret"}});
    let excerpt = excerpt_from_error_bytes(&serde_json::to_vec(&body).unwrap(), &[]).unwrap();
    for forbidden in [
        "\u{1b}",
        "[31m",
        "unknown-token",
        "unknown-key",
        "unknown-secret",
    ] {
        assert!(!excerpt.contains(forbidden), "leaked {forbidden:?}");
    }
    assert!(excerpt.contains("bad field"));
    let body = serde_json::json!({"error": {"message": "rejected fixture-runtime-credential"}});
    let excerpt = excerpt_from_error_bytes(
        &serde_json::to_vec(&body).unwrap(),
        &["fixture-runtime-credential"],
    )
    .unwrap();
    assert_eq!(excerpt, "rejected [REDACTED]");
}

#[tokio::test]
async fn chat_reasoning_continuation_is_bound_to_the_exact_runtime_endpoint() {
    use tokio::io::AsyncWriteExt;

    async fn serve(bodies: Vec<String>) -> (String, tokio::task::JoinHandle<Vec<Vec<u8>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for body in bodies {
                let (mut stream, _) = listener.accept().await.unwrap();
                requests.push(read_http_request(&mut stream).await);
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
                stream.write_all(body.as_bytes()).await.unwrap();
            }
            requests
        });
        (url, server)
    }

    fn client_for(endpoint: String) -> AgentClient {
        let model = "deepseek-v4-pro";
        AgentClient::with_policy(
            "deepseek",
            &ProviderConfig {
                google: None,
                tool_schema_policy: Default::default(),
                base_url: endpoint,
                api_key: String::new(),
                protocol: crate::agent::ProviderProtocol::ChatCompletions,
                prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                models: vec![model.into()],
                timeout: ProviderTimeoutConfig::default(),
            },
            model,
            fast_policy(),
            crate::request::RequestStore::default(),
        )
        .with_contract_for_test(official_contract(
            "deepseek",
            crate::agent::ProviderProtocol::ChatCompletions,
            model,
        ))
    }

    let exact_reasoning = "route-a opaque sentinel\n  λ  ";
    let tool_body = format!(
        "data: {{\"choices\":[{{\"delta\":{{\"reasoning_content\":{},\"tool_calls\":[{{\"index\":0,\"id\":\"call-route\",\"function\":{{\"name\":\"read_file\",\"arguments\":\"{{\\\"path\\\":\\\"input.txt\\\"}}\"}}}}]}},\"finish_reason\":\"tool_calls\"}}]}}\n\ndata: [DONE]\n\n",
        serde_json::to_string(exact_reasoning).unwrap()
    );
    let done_body =
        "data: {\"choices\":[{\"delta\":{\"content\":\"done\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".to_string();

    let (endpoint_a, server_a) = serve(vec![tool_body, done_body.clone()]).await;
    let client_a = client_for(endpoint_a.clone());
    let first = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::MainTurn,
        "deepseek",
        "deepseek-v4-pro",
        vec![ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text("read input.txt"),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        }],
        Vec::new(),
        ChatRequestOptions::default(),
    );
    let mut first_stream = client_a.stream_prepared(&first).await.unwrap();
    let first_turn = crate::agent::stream::consume_stream(&mut first_stream, |_| {})
        .await
        .unwrap();
    assert_eq!(first_turn.reasoning.as_deref(), Some(exact_reasoning));

    let continuation_messages = AgentClient::build_messages(
        None,
        &[
            user("read input.txt"),
            Message {
                role: Role::Assistant,
                content: first_turn.content,
                tool_calls: Some(vec![ToolCall {
                    id: first_turn.tool_calls[0].id.clone(),
                    name: first_turn.tool_calls[0].name.clone(),
                    arguments: first_turn.tool_calls[0].arguments.clone(),
                    status: ToolStatus::Done,
                    output: Some("hello".into()),
                }]),
                thinking: first_turn.reasoning,
                response_items: first_turn.response_items,
                timestamp: fixed_time(),
            },
        ],
        None,
        &[],
    )
    .unwrap();
    let continuation = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::ToolContinuation,
        "deepseek",
        "deepseek-v4-pro",
        continuation_messages,
        Vec::new(),
        ChatRequestOptions::default(),
    );

    let mut same_route_stream = client_a.stream_prepared(&continuation).await.unwrap();
    crate::agent::stream::consume_stream(&mut same_route_stream, |_| {})
        .await
        .unwrap();
    let route_a_requests = server_a.await.unwrap();
    assert_eq!(route_a_requests.len(), 2);
    let same_route: serde_json::Value =
        serde_json::from_slice(request_body(&route_a_requests[1])).unwrap();
    assert_eq!(
        same_route["messages"][1]["reasoning_content"],
        exact_reasoning
    );

    let (endpoint_b, server_b) = serve(vec![done_body]).await;
    assert_ne!(endpoint_a, endpoint_b);
    let client_b = client_for(endpoint_b);
    let mut changed_route_stream = client_b.stream_prepared(&continuation).await.unwrap();
    crate::agent::stream::consume_stream(&mut changed_route_stream, |_| {})
        .await
        .unwrap();
    let route_b_requests = server_b.await.unwrap();
    assert_eq!(route_b_requests.len(), 1);
    let changed_route: serde_json::Value =
        serde_json::from_slice(request_body(&route_b_requests[0])).unwrap();
    assert!(changed_route["messages"][1]
        .get("reasoning_content")
        .is_none());
    assert_eq!(
        changed_route["messages"][1]["tool_calls"][0]["id"],
        "call-route"
    );
    assert_eq!(changed_route["messages"][2]["tool_call_id"], "call-route");
    assert!(!String::from_utf8_lossy(request_body(&route_b_requests[0])).contains(exact_reasoning));
}

async fn capture_chat_completions_request(api_key: &str) -> Vec<u8> {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        let body =
            b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream.write_all(body).await.unwrap();
        request
    });
    let client = AgentClient::with_policy(
        "test",
        &ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: url,
            api_key: api_key.into(),
            protocol: crate::agent::ProviderProtocol::default(),
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["model".into()],
            timeout: ProviderTimeoutConfig::default(),
        },
        "model",
        fast_policy(),
        crate::request::RequestStore::default(),
    );
    let mut receiver = client
        .chat_stream(
            &[ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text("hello"),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            }],
            &[],
        )
        .await
        .unwrap();
    crate::agent::stream::consume_stream(&mut receiver, |_| {})
        .await
        .unwrap();
    server.await.unwrap()
}

async fn capture_openai_responses_request(api_key: &str) -> Vec<u8> {
    capture_openai_responses_request_with_account(api_key, None).await
}

async fn capture_openai_responses_request_with_account(
    api_key: &str,
    account_id: Option<&str>,
) -> Vec<u8> {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        let body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"content_index\":0,\"delta\":\"OK\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream.write_all(body.as_bytes()).await.unwrap();
        request
    });
    let request_store = crate::request::RequestStore::default();
    let mut client = AgentClient::with_policy(
        "test",
        &ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: url,
            api_key: api_key.into(),
            protocol: crate::agent::ProviderProtocol::OpenAiResponses,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["model".into()],
            timeout: ProviderTimeoutConfig::default(),
        },
        "model",
        fast_policy(),
        request_store.clone(),
    );
    if let Some(account_id) = account_id {
        client = client.with_codex_account_id_for_test(account_id);
    }
    let mut receiver = client
        .chat_stream(
            &[ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text("hello"),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            }],
            &[],
        )
        .await
        .unwrap();
    let turn = crate::agent::stream::consume_stream(&mut receiver, |_| {})
        .await
        .unwrap();
    assert_eq!(turn.content, "OK");
    assert_eq!(turn.usage.unwrap().total_tokens, 2);
    assert!(request_store
        .snapshot()
        .unwrap()
        .render_text()
        .contains("openai_responses"));
    server.await.unwrap()
}

async fn capture_openai_responses_reasoning_tool_loop() -> (Vec<Vec<u8>>, String) {
    use tokio::io::AsyncWriteExt;

    fn sse(events: &[serde_json::Value]) -> String {
        events
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect()
    }

    let reasoning_item = serde_json::json!({
        "id": "rs_loop",
        "type": "reasoning",
        "summary": [{"type": "summary_text", "text": "inspect the file"}],
        "encrypted_content": "sentinel-encrypted-loop-state",
        "status": "completed"
    });
    let message_item = serde_json::json!({
        "id": "msg_loop",
        "type": "message",
        "role": "assistant",
        "content": [{
            "type": "output_text",
            "text": "I will inspect it.",
            "annotations": []
        }],
        "status": "completed"
    });
    let function_call_item = serde_json::json!({
        "id": "fc_loop",
        "type": "function_call",
        "call_id": "call-loop",
        "name": "read_file",
        "arguments": "{\"path\":\"input.txt\"}",
        "status": "completed"
    });
    let first_body = sse(&[
        serde_json::json!({
            "type": "response.reasoning_summary_text.delta",
            "output_index": 0,
            "summary_index": 0,
            "delta": "inspect the file"
        }),
        serde_json::json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": reasoning_item.clone()
        }),
        serde_json::json!({
            "type": "response.output_text.delta",
            "output_index": 1,
            "content_index": 0,
            "delta": "I will inspect it."
        }),
        serde_json::json!({
            "type": "response.output_item.done",
            "output_index": 1,
            "item": message_item.clone()
        }),
        serde_json::json!({
            "type": "response.output_item.added",
            "output_index": 2,
            "item": {
                "type": "function_call",
                "call_id": "call-loop",
                "name": "read_file",
                "arguments": ""
            }
        }),
        serde_json::json!({
            "type": "response.output_item.done",
            "output_index": 2,
            "item": function_call_item.clone()
        }),
        serde_json::json!({
            "type": "response.completed",
            "response": {
                "status": "completed",
                "output": [
                    reasoning_item.clone(),
                    message_item.clone(),
                    function_call_item.clone()
                ],
                "usage": {"input_tokens": 4, "output_tokens": 3, "total_tokens": 7}
            }
        }),
    ]);
    let final_message_item = serde_json::json!({
        "id": "msg_done",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "output_text", "text": "done", "annotations": []}],
        "status": "completed"
    });
    let second_body = sse(&[
        serde_json::json!({
            "type": "response.output_text.delta",
            "output_index": 0,
            "content_index": 0,
            "delta": "done"
        }),
        serde_json::json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": final_message_item.clone()
        }),
        serde_json::json!({
            "type": "response.completed",
            "response": {
                "status": "completed",
                "output": [final_message_item],
                "usage": {"input_tokens": 8, "output_tokens": 1, "total_tokens": 9}
            }
        }),
    ]);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for body in [first_body, second_body] {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(read_http_request(&mut stream).await);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(body.as_bytes()).await.unwrap();
        }
        requests
    });
    let request_store = crate::request::RequestStore::default();
    let client = AgentClient::with_policy(
        crate::config::OPENAI_PROVIDER_TEMPLATE_ID,
        &ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: url,
            api_key: String::new(),
            protocol: crate::agent::ProviderProtocol::OpenAiResponses,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["gpt-5.6-sol".into()],
            timeout: ProviderTimeoutConfig::default(),
        },
        "gpt-5.6-sol",
        fast_policy(),
        request_store.clone(),
    )
    .with_contract_for_test(official_contract(
        crate::config::OPENAI_PROVIDER_TEMPLATE_ID,
        crate::agent::ProviderProtocol::OpenAiResponses,
        "gpt-5.6-sol",
    ));
    let first = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::MainTurn,
        crate::config::OPENAI_PROVIDER_TEMPLATE_ID,
        "gpt-5.6-sol",
        vec![ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text("read input.txt"),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        }],
        Vec::new(),
        ChatRequestOptions::default(),
    );
    let mut first_stream = client.stream_prepared(&first).await.unwrap();
    let first_turn = crate::agent::stream::consume_stream(&mut first_stream, |_| {})
        .await
        .unwrap();
    assert_eq!(first_turn.content, "I will inspect it.");
    assert_eq!(first_turn.reasoning.as_deref(), Some("inspect the file"));
    assert_eq!(first_turn.response_items.len(), 3);
    assert_eq!(first_turn.tool_calls.len(), 1);

    let continuation_state = vec![
        user("read input.txt"),
        Message {
            role: Role::Assistant,
            content: first_turn.content,
            tool_calls: Some(vec![ToolCall {
                id: first_turn.tool_calls[0].id.clone(),
                name: first_turn.tool_calls[0].name.clone(),
                arguments: first_turn.tool_calls[0].arguments.clone(),
                status: ToolStatus::Done,
                output: Some("hello".into()),
            }]),
            thinking: first_turn.reasoning,
            response_items: first_turn.response_items,
            timestamp: fixed_time(),
        },
    ];
    let continuation_messages =
        AgentClient::build_messages(None, &continuation_state, None, &[]).unwrap();
    let continuation = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::ToolContinuation,
        crate::config::OPENAI_PROVIDER_TEMPLATE_ID,
        "gpt-5.6-sol",
        continuation_messages,
        Vec::new(),
        ChatRequestOptions::default(),
    );
    let mut second_stream = client.stream_prepared(&continuation).await.unwrap();
    let second_turn = crate::agent::stream::consume_stream(&mut second_stream, |_| {})
        .await
        .unwrap();
    assert_eq!(second_turn.content, "done");

    let snapshot = request_store.snapshot().unwrap().render_text();
    (server.await.unwrap(), snapshot)
}

async fn capture_first_party_responses_tool_loop(provider: &str, model: &str) -> Vec<Vec<u8>> {
    use tokio::io::AsyncWriteExt;

    fn sse(events: &[serde_json::Value]) -> String {
        events
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect()
    }

    let message_item = serde_json::json!({
        "id": "msg_loop",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "output_text", "text": "checking", "annotations": []}],
        "status": "completed"
    });
    let function_call_item = serde_json::json!({
        "id": "fc_loop",
        "type": "function_call",
        "call_id": "call-loop",
        "name": "read_file",
        "arguments": "{\"path\":\"input.txt\"}",
        "status": "completed"
    });
    let first_body = sse(&[
        serde_json::json!({
            "type": "response.output_text.delta",
            "output_index": 0,
            "content_index": 0,
            "delta": "checking"
        }),
        serde_json::json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": message_item.clone()
        }),
        serde_json::json!({
            "type": "response.output_item.added",
            "output_index": 1,
            "item": {
                "type": "function_call",
                "call_id": "call-loop",
                "name": "read_file",
                "arguments": ""
            }
        }),
        serde_json::json!({
            "type": "response.output_item.done",
            "output_index": 1,
            "item": function_call_item.clone()
        }),
        serde_json::json!({
            "type": "response.completed",
            "response": {
                "status": "completed",
                "output": [message_item.clone(), function_call_item.clone()],
                "usage": {
                    "input_tokens": 4,
                    "output_tokens": 2,
                    "total_tokens": 6,
                    "input_tokens_details": {"cached_tokens": 0}
                }
            }
        }),
    ]);
    let final_message_item = serde_json::json!({
        "id": "msg_done",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "output_text", "text": "done", "annotations": []}],
        "status": "completed"
    });
    let second_body = sse(&[
        serde_json::json!({
            "type": "response.output_text.delta",
            "output_index": 0,
            "content_index": 0,
            "delta": "done"
        }),
        serde_json::json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": final_message_item.clone()
        }),
        serde_json::json!({
            "type": "response.completed",
            "response": {
                "status": "completed",
                "output": [final_message_item],
                "usage": {
                    "input_tokens": 8,
                    "output_tokens": 1,
                    "total_tokens": 9,
                    "input_tokens_details": {"cached_tokens": 2}
                }
            }
        }),
    ]);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for body in [first_body, second_body] {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(read_http_request(&mut stream).await);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(body.as_bytes()).await.unwrap();
        }
        requests
    });
    let client = AgentClient::with_policy(
        provider,
        &ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: url,
            api_key: String::new(),
            protocol: crate::agent::ProviderProtocol::OpenAiResponses,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec![model.into()],
            timeout: ProviderTimeoutConfig::default(),
        },
        model,
        fast_policy(),
        crate::request::RequestStore::default(),
    )
    .with_contract_for_test(official_contract(
        provider,
        crate::agent::ProviderProtocol::OpenAiResponses,
        model,
    ));
    let options = ChatRequestOptions {
        reasoning_effort: Some(ReasoningEffort::High),
        reasoning_request_format: Some(ReasoningRequestFormat::ReasoningObject),
        reasoning_stream_field: None,
        tool_call_format: None,
    };
    let first = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::MainTurn,
        provider,
        model,
        vec![ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text("read input.txt"),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        }],
        Vec::new(),
        options.clone(),
    );
    let mut first_stream = client.stream_prepared(&first).await.unwrap();
    let first_turn = crate::agent::stream::consume_stream(&mut first_stream, |_| {})
        .await
        .unwrap();
    assert_eq!(first_turn.tool_calls.len(), 1);
    assert_eq!(
        first_turn.usage.as_ref().unwrap().cached_prompt_tokens,
        Some(0)
    );

    let continuation_state = vec![
        user("read input.txt"),
        Message {
            role: Role::Assistant,
            content: first_turn.content,
            tool_calls: Some(vec![ToolCall {
                id: first_turn.tool_calls[0].id.clone(),
                name: first_turn.tool_calls[0].name.clone(),
                arguments: first_turn.tool_calls[0].arguments.clone(),
                status: ToolStatus::Done,
                output: Some("hello".into()),
            }]),
            thinking: first_turn.reasoning,
            response_items: first_turn.response_items,
            timestamp: fixed_time(),
        },
    ];
    let continuation = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::ToolContinuation,
        provider,
        model,
        AgentClient::build_messages(None, &continuation_state, None, &[]).unwrap(),
        Vec::new(),
        options,
    );
    let mut second_stream = client.stream_prepared(&continuation).await.unwrap();
    let second_turn = crate::agent::stream::consume_stream(&mut second_stream, |_| {})
        .await
        .unwrap();
    assert_eq!(second_turn.content, "done");
    assert_eq!(second_turn.usage.unwrap().cached_prompt_tokens, Some(2));
    server.await.unwrap()
}

async fn capture_anthropic_messages_request(api_key: &str) -> Vec<u8> {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        let body = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"OK\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream.write_all(body.as_bytes()).await.unwrap();
        request
    });
    let request_store = crate::request::RequestStore::default();
    let client = AgentClient::with_policy(
        "test",
        &ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: url,
            api_key: api_key.into(),
            protocol: crate::agent::ProviderProtocol::AnthropicMessages,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["model".into()],
            timeout: ProviderTimeoutConfig::default(),
        },
        "model",
        fast_policy(),
        request_store.clone(),
    );
    let mut receiver = client
        .chat_stream(
            &[ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text("hello"),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            }],
            &[],
        )
        .await
        .unwrap();
    let turn = crate::agent::stream::consume_stream(&mut receiver, |_| {})
        .await
        .unwrap();
    assert_eq!(turn.content, "OK");
    assert_eq!(turn.usage.unwrap().total_tokens, 2);
    assert!(request_store
        .snapshot()
        .unwrap()
        .render_text()
        .contains("anthropic_messages"));
    server.await.unwrap()
}

#[tokio::test]
async fn chat_completions_adapter_preserves_path_auth_and_exact_body() {
    let authenticated = capture_chat_completions_request("sentinel-wire-key").await;
    let authenticated_text = String::from_utf8_lossy(&authenticated);
    assert!(authenticated_text.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
    assert!(authenticated_text
        .to_ascii_lowercase()
        .contains("content-type: application/json\r\n"));
    assert!(authenticated_text.contains("authorization: Bearer sentinel-wire-key\r\n"));
    assert_eq!(
        request_body(&authenticated),
        br#"{"model":"model","messages":[{"role":"user","content":"hello"}],"stream":true}"#
    );

    let credentialless = capture_chat_completions_request("").await;
    assert!(!String::from_utf8_lossy(&credentialless)
        .to_ascii_lowercase()
        .contains("authorization:"));
    assert_eq!(request_body(&credentialless), request_body(&authenticated));
}

#[tokio::test]
async fn openai_responses_adapter_preserves_path_auth_and_exact_body() {
    let authenticated = capture_openai_responses_request("sentinel-wire-key").await;
    let authenticated_text = String::from_utf8_lossy(&authenticated);
    assert!(authenticated_text.starts_with("POST /v1/responses HTTP/1.1\r\n"));
    assert!(authenticated_text.contains("authorization: Bearer sentinel-wire-key\r\n"));
    assert_eq!(
        request_body(&authenticated),
        br#"{"model":"model","input":[{"role":"user","content":"hello"}],"stream":true}"#
    );

    let credentialless = capture_openai_responses_request("").await;
    assert!(!String::from_utf8_lossy(&credentialless)
        .to_ascii_lowercase()
        .contains("authorization:"));
    assert_eq!(request_body(&credentialless), request_body(&authenticated));
}

#[tokio::test]
async fn codex_account_header_is_applied_without_entering_request_snapshots() {
    let request = capture_openai_responses_request_with_account(
        "sentinel-wire-key",
        Some("sentinel-account-id"),
    )
    .await;
    let request_text = String::from_utf8_lossy(&request).to_ascii_lowercase();
    assert!(request_text.contains("chatgpt-account-id: sentinel-account-id\r\n"));
}

#[tokio::test]
async fn openai_responses_two_request_loop_replays_complete_output_sequence() {
    let (requests, snapshot) = capture_openai_responses_reasoning_tool_loop().await;
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert!(String::from_utf8_lossy(request).starts_with("POST /v1/responses HTTP/1.1\r\n"));
        let body: serde_json::Value = serde_json::from_slice(request_body(request)).unwrap();
        assert_eq!(
            body["include"],
            serde_json::json!(["reasoning.encrypted_content"])
        );
        assert!(body.get("previous_response_id").is_none());
    }

    let continuation: serde_json::Value =
        serde_json::from_slice(request_body(&requests[1])).unwrap();
    let input = continuation["input"].as_array().unwrap();
    assert_eq!(input.len(), 5);
    assert_eq!(input[0]["role"], "user");
    assert_eq!(input[1]["type"], "reasoning");
    assert_eq!(input[1]["id"], "rs_loop");
    assert_eq!(
        input[1]["encrypted_content"],
        "sentinel-encrypted-loop-state"
    );
    assert_eq!(input[2]["type"], "message");
    assert_eq!(input[2]["content"][0]["text"], "I will inspect it.");
    assert_eq!(input[3]["type"], "function_call");
    assert_eq!(input[3]["id"], "fc_loop");
    assert_eq!(input[3]["call_id"], "call-loop");
    assert_eq!(input[4]["type"], "function_call_output");
    assert_eq!(input[4]["call_id"], "call-loop");
    assert_eq!(input[4]["output"], "hello");

    assert!(!snapshot.contains("sentinel-encrypted-loop-state"));
    assert!(!snapshot.contains("rs_loop"));
}

#[tokio::test]
async fn mimo_and_deepseek_responses_two_request_loops_replay_without_openai_fields() {
    for (provider, model) in [("mimo", "mimo-v2.5-pro"), ("deepseek", "deepseek-v4-pro")] {
        let requests = capture_first_party_responses_tool_loop(provider, model).await;
        assert_eq!(requests.len(), 2);
        for request in &requests {
            let body: serde_json::Value = serde_json::from_slice(request_body(request)).unwrap();
            assert_eq!(body["reasoning"]["effort"], "high");
            for foreign in [
                "include",
                "store",
                "prompt_cache_key",
                "prompt_cache_options",
                "previous_response_id",
            ] {
                assert!(body.get(foreign).is_none(), "{provider} sent {foreign}");
            }
        }
        let continuation: serde_json::Value =
            serde_json::from_slice(request_body(&requests[1])).unwrap();
        let input = continuation["input"].as_array().unwrap();
        assert_eq!(input.len(), 4);
        assert_eq!(input[1]["type"], "message");
        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[3]["type"], "function_call_output");
    }
}

#[tokio::test]
async fn anthropic_messages_adapter_preserves_path_headers_and_exact_body() {
    let authenticated = capture_anthropic_messages_request("sentinel-wire-key").await;
    let authenticated_text = String::from_utf8_lossy(&authenticated);
    assert!(authenticated_text.starts_with("POST /v1/messages HTTP/1.1\r\n"));
    assert!(authenticated_text.contains("x-api-key: sentinel-wire-key\r\n"));
    assert!(authenticated_text.contains("anthropic-version: 2023-06-01\r\n"));
    assert!(!authenticated_text
        .to_ascii_lowercase()
        .contains("authorization:"));
    assert_eq!(
        request_body(&authenticated),
        br#"{"model":"model","max_tokens":16384,"messages":[{"role":"user","content":[{"type":"text","text":"hello"}]}],"stream":true}"#
    );

    let credentialless = capture_anthropic_messages_request("").await;
    let credentialless_text = String::from_utf8_lossy(&credentialless);
    assert!(!credentialless_text
        .to_ascii_lowercase()
        .contains("x-api-key:"));
    assert!(credentialless_text.contains("anthropic-version: 2023-06-01\r\n"));
    assert_eq!(request_body(&credentialless), request_body(&authenticated));
}

#[tokio::test]
async fn openai_responses_non_text_input_fails_before_network_io() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let client = AgentClient::with_policy(
        "test",
        &ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: url,
            api_key: String::new(),
            protocol: crate::agent::ProviderProtocol::OpenAiResponses,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["model".into()],
            timeout: ProviderTimeoutConfig::default(),
        },
        "model",
        fast_policy(),
        crate::request::RequestStore::default(),
    );
    let error = client
        .chat_stream(
            &[ChatMessage {
                role: "assistant".into(),
                content: crate::image_content::ModelContent::multimodal(
                    "sentinel",
                    &[crate::image_content::normalize_rgba(1, 1, vec![0, 0, 0, 255]).unwrap()],
                ),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            }],
            &[],
        )
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Protocol(_)));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(40), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn openai_responses_timeout_remains_typed() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_request(&mut stream).await;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 100\r\n\r\n",
            )
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    });
    let mut receiver = test_client_with_protocol(
        url,
        fast_policy(),
        crate::agent::ProviderProtocol::OpenAiResponses,
    )
    .chat_stream(&[], &[])
    .await
    .unwrap();

    assert_eq!(
        receiver.recv().await.unwrap().unwrap_err(),
        ProviderError::Transport(ProviderTransportFailure::FirstTokenTimeout)
    );
    server.await.unwrap();
}

#[tokio::test]
async fn anthropic_messages_timeout_remains_typed() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_request(&mut stream).await;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 100\r\n\r\n",
            )
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    });
    let mut receiver = test_client_with_protocol(
        url,
        fast_policy(),
        crate::agent::ProviderProtocol::AnthropicMessages,
    )
    .chat_stream(&[], &[])
    .await
    .unwrap();

    assert_eq!(
        receiver.recv().await.unwrap().unwrap_err(),
        ProviderError::Transport(ProviderTransportFailure::FirstTokenTimeout)
    );
    server.await.unwrap();
}

#[tokio::test]
async fn repetition_guard_preserves_prior_unicode_output_cancels_and_never_replays() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let span = "A bounded explanation with Unicode 🙂 repeats because generation is stuck. ";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server_span = span.to_string();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let mut body = Vec::new();
        for _ in 0..3 {
            body.extend_from_slice(
                format!(
                    "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}},\"finish_reason\":null}}]}}\n\n",
                    serde_json::to_string(&server_span).unwrap()
                )
                .as_bytes(),
            );
        }
        body.extend_from_slice(
            b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        );
        for byte in body {
            if stream.write_all(&[byte]).await.is_err() {
                break;
            }
            tokio::task::yield_now().await;
        }
        let mut probe = [0_u8; 1];
        let disconnected = matches!(
            tokio::time::timeout(Duration::from_secs(1), stream.read(&mut probe)).await,
            Ok(Ok(0)) | Ok(Err(_))
        );
        let no_replay = tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err();
        (disconnected, no_replay)
    });

    let mut policy = fast_policy();
    // This case asserts cancellation/replay semantics, not scheduler latency.
    policy.connect_timeout = Duration::from_secs(5);
    policy.headers_timeout = Duration::from_secs(5);
    policy.first_event_timeout = Duration::from_secs(5);
    policy.no_progress_timeout = Duration::from_secs(5);
    policy.max_attempts = 2;
    let mut receiver = test_client(url, policy)
        .chat_stream(&[], &[])
        .await
        .unwrap();
    let mut delivered = String::new();
    let error = crate::agent::stream::consume_stream(&mut receiver, |event| {
        if let ProviderEvent::Text(text) = event {
            delivered.push_str(text);
        }
    })
    .await
    .unwrap_err();
    assert_eq!(
        error,
        ProviderError::StreamAnomaly(crate::agent::stream::StreamAnomalyKind::RepetitionLoop)
    );
    assert_eq!(delivered, format!("{span}{span}"));
    drop(receiver);
    let (disconnected, no_replay) = server.await.unwrap();
    assert!(disconnected, "an anomaly must drop the live response body");
    assert!(no_replay, "usable streamed output must never be replayed");
}

#[tokio::test]
async fn pseudo_tool_text_is_preserved_but_never_becomes_a_tool_event() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"{\\\"name\\\":\\\"read_file\\\",\\\"arguments\\\":{}}\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    });
    let tool = ToolDef {
        def_type: "function".into(),
        function: ToolFunction {
            name: "read_file".into(),
            description: "Read a contained file".into(),
            parameters: serde_json::json!({"type":"object"}),
        },
    };
    let mut receiver = test_client(url, fast_policy())
        .chat_stream(&[], &[tool])
        .await
        .unwrap();
    let mut delivered = Vec::new();
    let error = crate::agent::stream::consume_stream(&mut receiver, |event| {
        delivered.push(event.clone());
    })
    .await
    .unwrap_err();
    assert_eq!(
        error,
        ProviderError::StreamAnomaly(
            crate::agent::stream::StreamAnomalyKind::SuspectedToolEnvelope
        )
    );
    assert_eq!(
        delivered,
        [ProviderEvent::Text(
            r#"{"name":"read_file","arguments":{}}"#.into()
        )]
    );
    server.await.unwrap();
}

async fn dsml_loopback(
    provider_id: &str,
    explicit_format: Option<crate::config::ToolCallFormat>,
    first_party: bool,
) -> Result<
    (
        crate::agent::stream::AssembledAssistantTurn,
        Vec<ProviderEvent>,
    ),
    ProviderError,
> {
    use tokio::io::AsyncWriteExt;

    let payload = concat!(
        "<｜DSML｜tool_calls>\n",
        "<｜DSML｜invoke name=\"propose_plan\">\n",
        "<｜DSML｜parameter name=\"expected_revision\" string=\"false\">5</｜DSML｜parameter>\n",
        "<｜DSML｜parameter name=\"title\" string=\"true\">Protected-browser correctness</｜DSML｜parameter>\n",
        "</｜DSML｜invoke>\n",
        "</｜DSML｜tool_calls>",
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        let body = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}},\"finish_reason\":null}}]}}\n\ndata: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n",
            serde_json::to_string(payload).unwrap()
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        for byte in body.bytes() {
            if stream.write_all(&[byte]).await.is_err() {
                break;
            }
            tokio::task::yield_now().await;
        }
    });
    let model = "deepseek-v4-pro";
    let mut client = AgentClient::with_policy(
        provider_id,
        &ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: url,
            api_key: String::new(),
            protocol: crate::agent::ProviderProtocol::ChatCompletions,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec![model.into()],
            timeout: ProviderTimeoutConfig::default(),
        },
        model,
        fast_policy(),
        crate::request::RequestStore::default(),
    );
    if first_party {
        client = client.with_contract_for_test(official_contract(
            "deepseek",
            crate::agent::ProviderProtocol::ChatCompletions,
            model,
        ));
    }
    let request = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::MainTurn,
        provider_id,
        model,
        vec![ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text("make a plan"),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        }],
        vec![ToolDef {
            def_type: "function".into(),
            function: ToolFunction {
                name: "propose_plan".into(),
                description: "Propose a plan".into(),
                parameters: serde_json::json!({"type":"object"}),
            },
        }],
        ChatRequestOptions {
            tool_call_format: explicit_format,
            ..ChatRequestOptions::default()
        },
    );
    let mut receiver = client.stream_prepared(&request).await.unwrap();
    let mut delivered = Vec::new();
    let result = crate::agent::stream::consume_stream(&mut receiver, |event| {
        delivered.push(event.clone());
    })
    .await;
    server.await.unwrap();
    result.map(|turn| (turn, delivered))
}

#[tokio::test]
async fn deepseek_dsml_loopback_is_automatic_opt_in_capable_and_generic_fail_closed() {
    for (provider, format, first_party) in [
        ("deepseek", None, true),
        (
            "proxy",
            Some(crate::config::ToolCallFormat::DeepseekDsml),
            false,
        ),
    ] {
        let (turn, delivered) = dsml_loopback(provider, format, first_party).await.unwrap();
        assert!(turn.content.is_empty());
        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(turn.tool_calls[0].name, "propose_plan");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&turn.tool_calls[0].arguments).unwrap(),
            serde_json::json!({
                "expected_revision": 5,
                "title": "Protected-browser correctness"
            })
        );
        assert!(!delivered
            .iter()
            .any(|event| matches!(event, ProviderEvent::Text(_))));
    }

    assert_eq!(
        dsml_loopback("generic", None, false).await.unwrap_err(),
        ProviderError::StreamAnomaly(
            crate::agent::stream::StreamAnomalyKind::SuspectedToolEnvelope
        )
    );
}

async fn assert_dropping_receiver_cancels_body_consumption(
    protocol: crate::agent::ProviderProtocol,
    first_event: &'static [u8],
    next_event: &'static [u8],
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 100000\r\n\r\n",
            )
            .await
            .unwrap();
        stream.write_all(first_event).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let _ = stream.write_all(next_event).await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let _ = stream.write_all(next_event).await;
        let mut byte = [0_u8; 1];
        matches!(
            tokio::time::timeout(
                std::time::Duration::from_millis(500),
                stream.read(&mut byte)
            )
            .await,
            Ok(Ok(0)) | Ok(Err(_))
        )
    });
    // FreeBSD's loopback stack can legitimately cross the deliberately tiny
    // generic timeout fixture while acknowledging the first short TCP write.
    // This test is about receiver-drop cancellation, not deadline precision.
    let mut policy = fast_policy();
    policy.first_event_timeout = std::time::Duration::from_millis(500);
    let mut receiver = test_client_with_protocol(url, policy, protocol)
        .chat_stream(&[], &[])
        .await
        .unwrap();
    assert_eq!(
        receiver.recv().await.unwrap().unwrap(),
        ProviderEvent::Text("x".into())
    );
    drop(receiver);

    assert!(
        server.await.unwrap(),
        "dropping the receiver must close the body"
    );
}

#[tokio::test]
async fn dropping_protocol_receivers_cancels_body_consumption() {
    assert_dropping_receiver_cancels_body_consumption(
        crate::agent::ProviderProtocol::OpenAiResponses,
        b"data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"content_index\":0,\"delta\":\"x\"}\n\n",
        b"data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"content_index\":0,\"delta\":\"x\"}\n\n",
    )
    .await;
    assert_dropping_receiver_cancels_body_consumption(
        crate::agent::ProviderProtocol::AnthropicMessages,
        concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"content\":[],\"usage\":{\"input_tokens\":1}}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"x\"}}\n\n"
        )
        .as_bytes(),
        b"data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"x\"}}\n\n",
    )
    .await;
}

#[tokio::test]
async fn http_failure_discards_response_body_and_keeps_snapshot_safe() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        let body = b"sentinel-response-body";
        stream
            .write_all(
                format!(
                    "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream.write_all(body).await.unwrap();
    });
    let request_store = crate::request::RequestStore::default();
    let client = AgentClient::with_policy(
        "safe-provider",
        &ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: url,
            api_key: "sentinel-runtime-key".into(),
            protocol: crate::agent::ProviderProtocol::default(),
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["model".into()],
            timeout: ProviderTimeoutConfig::default(),
        },
        "model",
        fast_policy(),
        request_store.clone(),
    );

    let error = client.chat_stream(&[], &[]).await.unwrap_err();

    assert_eq!(
        error,
        ProviderError::Http {
            status: 401,
            body_excerpt: None
        }
    );
    let visible = format!(
        "{error}\n{client:?}\n{}",
        request_store.snapshot().unwrap().render_text()
    );
    for forbidden in [
        "sentinel-response-body",
        "sentinel-runtime-key",
        "authorization",
    ] {
        assert!(!visible.to_ascii_lowercase().contains(forbidden));
    }
    server.await.unwrap();
}

#[tokio::test]
async fn headers_and_first_event_deadlines_are_typed_timeouts() {
    use tokio::io::AsyncWriteExt;

    let headers_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let headers_url = format!("http://{}/v1", headers_listener.local_addr().unwrap());
    let headers_server = tokio::spawn(async move {
        let (mut stream, _) = headers_listener.accept().await.unwrap();
        read_request(&mut stream).await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    });
    let error = test_client(headers_url, fast_policy())
        .chat_stream(&[], &[])
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ProviderError::Transport(ProviderTransportFailure::Timeout)
    );
    headers_server.await.unwrap();

    let event_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let event_url = format!("http://{}/v1", event_listener.local_addr().unwrap());
    let event_server = tokio::spawn(async move {
        let (mut stream, _) = event_listener.accept().await.unwrap();
        read_request(&mut stream).await;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 100\r\n\r\n",
            )
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    });
    let mut receiver = test_client(event_url, fast_policy())
        .chat_stream(&[], &[])
        .await
        .unwrap();
    assert_eq!(
        receiver.recv().await.unwrap().unwrap_err(),
        ProviderError::Transport(ProviderTransportFailure::FirstTokenTimeout)
    );
    event_server.await.unwrap();
}

#[tokio::test]
async fn sse_keepalives_extend_no_progress_deadline() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_request(&mut stream).await;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 100000\r\n\r\n",
            )
            .await
            .unwrap();
        // Keep-alives must count as liveness even though they decode to zero events.
        // Window exceeds first_event_timeout so undecoded bytes must re-arm the deadline.
        for _ in 0..10 {
            stream.write_all(b": ping\n\n").await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
        let body = b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        let _ = stream.write_all(body).await;
    });

    let mut policy = fast_policy();
    policy.first_event_timeout = std::time::Duration::from_millis(250);
    policy.no_progress_timeout = std::time::Duration::from_millis(120);
    policy.max_attempts = 1;
    let mut receiver = test_client(url, policy)
        .chat_stream(&[], &[])
        .await
        .unwrap();
    let mut texts = Vec::new();
    loop {
        match receiver.recv().await {
            Some(Ok(ProviderEvent::Text(text))) => texts.push(text),
            Some(Ok(ProviderEvent::Done)) | None => break,
            Some(Ok(_)) => {}
            Some(Err(error)) => panic!("keep-alives must not time out: {error:?}"),
        }
    }
    assert_eq!(texts, vec!["ok".to_string()]);
    server.await.unwrap();
}

#[tokio::test]
async fn mid_stream_stall_trips_no_progress_timeout() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_request(&mut stream).await;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 100\r\n\r\n",
            )
            .await
            .unwrap();
        let first =
            b"data: {\"choices\":[{\"delta\":{\"content\":\"x\"},\"finish_reason\":null}]}\n\n";
        stream.write_all(first).await.unwrap();
        // Silence after a real event must trip no_progress_timeout.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    });

    let mut policy = fast_policy();
    policy.first_event_timeout = std::time::Duration::from_millis(200);
    policy.no_progress_timeout = std::time::Duration::from_millis(40);
    policy.max_attempts = 1;
    let mut receiver = test_client(url, policy)
        .chat_stream(&[], &[])
        .await
        .unwrap();
    assert_eq!(
        receiver.recv().await.unwrap().unwrap(),
        ProviderEvent::Text("x".into())
    );
    assert_eq!(
        receiver.recv().await.unwrap().unwrap_err(),
        ProviderError::Transport(ProviderTransportFailure::Timeout)
    );
    server.await.unwrap();
}

#[tokio::test]
async fn reasoning_deltas_extend_deadline_even_without_profile() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_request(&mut stream).await;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 100000\r\n\r\n",
            )
            .await
            .unwrap();
        // Long thinking preamble of reasoning_content deltas (no profile field
        // configured on this generic test client). Bytes must keep the stream alive
        // even when the decoder emits zero events. Window exceeds first_event_timeout.
        for _ in 0..10 {
            let event = b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"think \"},\"finish_reason\":null}]}\n\n";
            stream.write_all(event).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
        let body = b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        let _ = stream.write_all(body).await;
    });

    let mut policy = fast_policy();
    policy.first_event_timeout = std::time::Duration::from_millis(250);
    policy.no_progress_timeout = std::time::Duration::from_millis(120);
    policy.max_attempts = 1;
    let mut receiver = test_client(url, policy)
        .chat_stream(&[], &[])
        .await
        .unwrap();
    let mut texts = Vec::new();
    loop {
        match receiver.recv().await {
            Some(Ok(ProviderEvent::Text(text))) => texts.push(text),
            Some(Ok(ProviderEvent::Done)) | None => break,
            Some(Ok(_)) => {}
            Some(Err(error)) => panic!("reasoning bytes must not time out: {error:?}"),
        }
    }
    assert_eq!(texts, vec!["ok".to_string()]);
    server.await.unwrap();
}

async fn stalled_error_server(
    status: u16,
    prefix: Vec<u8>,
) -> (
    String,
    tokio::task::JoinHandle<bool>,
    tokio::sync::oneshot::Receiver<()>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let (sent, ready) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        stream
            .write_all(
                format!("HTTP/1.1 {status} Error\r\nContent-Length: 1000000000\r\n\r\n").as_bytes(),
            )
            .await
            .unwrap();
        stream.write_all(&prefix).await.unwrap();
        let _ = sent.send(());
        let mut byte = [0];
        matches!(
            tokio::time::timeout(Duration::from_secs(3), stream.read(&mut byte)).await,
            Ok(Ok(0)) | Ok(Err(_))
        )
    });
    (url, server, ready)
}

#[tokio::test]
async fn transport_regression_stalled_400_keeps_http_type() {
    let (url, server, _) = stalled_error_server(400, b"{\"error\":".to_vec()).await;
    let mut policy = fast_policy();
    policy.headers_timeout = Duration::from_millis(100);
    let error = tokio::time::timeout(
        Duration::from_millis(600),
        test_client(url, policy).chat_stream(&[], &[]),
    )
    .await
    .expect("error-body collection must respect headers_timeout")
    .unwrap_err();
    assert_eq!(error, ProviderError::http(400));
    assert!(
        server.await.unwrap(),
        "bounded collection must close the HTTP body"
    );
}

#[tokio::test]
async fn transport_regression_error_body_has_absolute_one_second_cap() {
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 1000000\r\n\r\n")
            .await
            .unwrap();
        for _ in 0..12 {
            if stream.write_all(b" ").await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    });
    let mut policy = fast_policy();
    policy.headers_timeout = Duration::from_secs(3);
    let error = tokio::time::timeout(
        Duration::from_millis(1500),
        test_client(url, policy).chat_stream(&[], &[]),
    )
    .await
    .expect("trickled error bytes must not renew the absolute one-second deadline")
    .unwrap_err();
    assert_eq!(error, ProviderError::http(400));
    server.await.unwrap();
}

#[tokio::test]
async fn transport_regression_error_body_stops_at_eight_kib() {
    let mut prefix = br#"{"error":{"message":"bounded prefix"}}"#.to_vec();
    prefix.resize(8 * 1024, b' ');
    let (url, server, _) = stalled_error_server(400, prefix).await;
    let mut policy = fast_policy();
    policy.headers_timeout = Duration::from_secs(3);
    let error = tokio::time::timeout(
        Duration::from_millis(600),
        test_client(url, policy).chat_stream(&[], &[]),
    )
    .await
    .expect("must stop collecting at 8KiB, before EOF or the time cap")
    .unwrap_err();
    assert_eq!(
        error,
        ProviderError::http_with_excerpt(400, "bounded prefix".to_string())
    );
    assert!(
        server.await.unwrap(),
        "must drop the unbounded response body"
    );
}

#[tokio::test]
async fn error_body_response_rule_stops_without_echoing_the_match() {
    use tokio::io::AsyncWriteExt;

    async fn serve_error() -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_http_request(&mut stream).await;
            let body = br#"{"error":{"message":"blocked NEEDLE value"}}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(body).await.unwrap();
        });
        (url, server)
    }

    let (url, server) = serve_error().await;
    let filter = std::sync::Arc::new(
        crate::provider_filter::ProviderFilter::compile(
            &crate::provider_filter::ProviderFilterConfig {
                enabled: true,
                trip_file: None,
                phrases: Vec::new(),
                urls: Vec::new(),
                rules: vec![crate::provider_filter::ProviderFilterRule {
                    id: "phrase".into(),
                    target: crate::provider_filter::ProviderFilterTarget::Response,
                    kind: crate::provider_filter::ProviderFilterKind::Literal,
                    pattern: "NEEDLE".into(),
                }],
                adjustments: Vec::new(),
                obfuscate: false,
                page_stand_in: crate::provider_filter::PageStandIn::Off,
                page_stand_ins: Vec::new(),
                placeholder_images: false,
                obfuscation_patterns: Vec::new(),

                stop: crate::provider_filter::FilterStop::default(),
                hide: crate::provider_filter::FilterHide::default(),
                pages: crate::provider_filter::PagesSetting::default(),
            },
        )
        .expect("filter compiles"),
    );
    let error = test_client(url, fast_policy())
        .with_filter_for_test(filter)
        .chat_stream(&[], &[])
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ProviderError::FilterStopped {
            rule: "phrase".into()
        }
    );
    assert!(!error.to_string().contains("NEEDLE"));
    server.await.unwrap();

    let (url, server) = serve_error().await;
    let filter = std::sync::Arc::new(
        crate::provider_filter::ProviderFilter::compile(
            &crate::provider_filter::ProviderFilterConfig {
                enabled: true,
                trip_file: None,
                phrases: Vec::new(),
                urls: Vec::new(),
                rules: vec![crate::provider_filter::ProviderFilterRule {
                    id: "phrase".into(),
                    target: crate::provider_filter::ProviderFilterTarget::Request,
                    kind: crate::provider_filter::ProviderFilterKind::Literal,
                    pattern: "NEEDLE".into(),
                }],
                adjustments: Vec::new(),
                obfuscate: false,
                page_stand_in: crate::provider_filter::PageStandIn::Off,
                page_stand_ins: Vec::new(),
                placeholder_images: false,
                obfuscation_patterns: Vec::new(),

                stop: crate::provider_filter::FilterStop::default(),
                hide: crate::provider_filter::FilterHide::default(),
                pages: crate::provider_filter::PagesSetting::default(),
            },
        )
        .expect("filter compiles"),
    );
    let error = test_client(url, fast_policy())
        .with_filter_for_test(filter)
        .chat_stream(&[], &[])
        .await
        .unwrap_err();
    match error {
        ProviderError::Http {
            status: 400,
            body_excerpt,
        } => assert_eq!(body_excerpt.as_deref(), Some("blocked NEEDLE value")),
        other => panic!("request rule must leave the error body excerpt, got {other:?}"),
    }
    server.await.unwrap();
}

fn response_filter(pattern: &str) -> std::sync::Arc<crate::provider_filter::ProviderFilter> {
    std::sync::Arc::new(
        crate::provider_filter::ProviderFilter::compile(
            &crate::provider_filter::ProviderFilterConfig {
                enabled: true,
                rules: vec![crate::provider_filter::ProviderFilterRule {
                    id: "response-stop".into(),
                    target: crate::provider_filter::ProviderFilterTarget::Response,
                    kind: crate::provider_filter::ProviderFilterKind::Literal,
                    pattern: pattern.into(),
                }],
                ..crate::provider_filter::ProviderFilterConfig::default()
            },
        )
        .unwrap(),
    )
}

async fn filtered_response_events(
    body: &str,
    pattern: &str,
) -> (Vec<ProviderEvent>, Option<ProviderError>) {
    filtered_response_events_with_filter(body, response_filter(pattern)).await
}

async fn filtered_response_events_with_filter(
    body: &str,
    filter: std::sync::Arc<crate::provider_filter::ProviderFilter>,
) -> (Vec<ProviderEvent>, Option<ProviderError>) {
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let body = body.to_owned();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    });
    let client = test_client(url, fast_policy()).with_filter_for_test(filter);
    let mut receiver = client
        .chat_stream_with_options(
            &[],
            &[],
            &ChatRequestOptions {
                reasoning_stream_field: Some("reasoning_content".into()),
                ..ChatRequestOptions::default()
            },
        )
        .await
        .unwrap();
    let mut events = Vec::new();
    let mut failure = None;
    while let Some(item) = receiver.recv().await {
        match item {
            Ok(event) => events.push(event),
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }
    server.await.unwrap();
    (events, failure)
}

#[tokio::test]
async fn filter_response_stops_decoded_text_across_frames_and_json_escapes() {
    for body in [
        "data: {\"choices\":[{\"delta\":{\"content\":\"sec\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"ret\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"sec\\u0072et\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
    ] {
        let (events, error) = filtered_response_events(body, "secret").await;
        assert_eq!(error, Some(ProviderError::FilterStopped { rule: "response-stop".into() }));
        assert!(!events.contains(&ProviderEvent::Text("ret".into())));
        assert!(!events.contains(&ProviderEvent::Text("secret".into())));
        assert!(!events.contains(&ProviderEvent::Done));
    }
}

#[tokio::test]
async fn filter_response_stops_decoded_reasoning_and_tool_arguments_across_frames() {
    for body in [
        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"sec\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"reasoning_content\":\"ret\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"sec\"}}]}}]}\n\ndata: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"ret\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n",
    ] {
        let (events, error) = filtered_response_events(body, "secret").await;
        assert_eq!(error, Some(ProviderError::FilterStopped { rule: "response-stop".into() }));
        assert!(!events.contains(&ProviderEvent::Done));
    }
}

#[tokio::test]
async fn filter_response_does_not_join_distinct_channels_or_tool_calls() {
    for body in [
        "data: {\"choices\":[{\"delta\":{\"content\":\"sec\",\"reasoning_content\":\"ret\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"name\":\"read_file\",\"arguments\":\"sec\"}},{\"index\":1,\"id\":\"call-2\",\"function\":{\"name\":\"read_file\",\"arguments\":\"ret\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n",
    ] {
        let (events, error) = filtered_response_events(body, "secret").await;
        assert_eq!(error, None);
        assert!(events.contains(&ProviderEvent::Done));
    }
}

#[tokio::test]
async fn filter_response_stops_escaped_tool_argument_values() {
    let body = concat!(
        r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-1","function":{"name":"read_file","arguments":"{\"path\":\"sec\\u0072et\"}"}}]},"finish_reason":"tool_calls"}]}"#,
        "\n\ndata: [DONE]\n\n"
    );
    let (events, error) = filtered_response_events(body, "secret").await;
    assert_eq!(
        error,
        Some(ProviderError::FilterStopped {
            rule: "response-stop".into()
        })
    );
    assert!(!events.contains(&ProviderEvent::Done));
}

fn tool_argument_sse(arguments: &[String]) -> String {
    let mut body = String::new();
    for (index, argument) in arguments.iter().enumerate() {
        let function = if index == 0 {
            serde_json::json!({"name":"read_file","arguments":argument})
        } else {
            serde_json::json!({"arguments":argument})
        };
        let mut call = serde_json::json!({"index":0,"function":function});
        if index == 0 {
            call["id"] = serde_json::json!("call-1");
        }
        body.push_str(&format!(
            "data: {}\n\n",
            serde_json::json!({"choices":[{"delta":{"tool_calls":[call]}}]})
        ));
    }
    body.push_str(
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n",
    );
    body
}

#[test]
fn filter_response_argument_lexer_tracks_activation_without_validating_disabled_json() {
    let idle = crate::provider_filter::ProviderFilter::compile(
        &crate::provider_filter::ProviderFilterConfig::default(),
    )
    .unwrap();
    for (prefix, suffix) in [
        ("{\"path\":\"safe-prefix-", "sec\\u0072et\"}"),
        ("{\"path\":\"\\uD8", "3D\\uDD12sec\\u0072et\"}"),
        ("{\"path\":\"\\uD83D", "\\uDD12sec\\u0072et\"}"),
        ("{\"path\":\"\\", "u0073ecret\"}"),
    ] {
        let mut arguments = FilteredToolArguments::default();
        arguments.observe(&idle, prefix).unwrap();
        assert_eq!(arguments.strings.fragment.capacity(), 0);
        assert_eq!(arguments.strings.decoded.capacity(), 0);
        assert!(matches!(
            arguments.observe(&response_filter("secret"), suffix),
            Err(ProviderError::FilterStopped { .. })
        ));
    }
    let mut arguments = FilteredToolArguments::default();
    arguments
        .observe(&idle, "{\"path\":\"bad\\q-unfiltered\"}")
        .unwrap();
    arguments.finish(&idle).unwrap();
    assert_eq!(arguments.strings.fragment.capacity(), 0);
    assert_eq!(arguments.strings.decoded.capacity(), 0);
}

#[tokio::test]
async fn filter_response_stops_long_escaped_tool_arguments_without_retaining_the_object() {
    for arguments in [
        vec![
            format!("{{\"pad\":\"{}", "x".repeat(9000)),
            "\",\"path\":\"sec\\u0072et\"}".into(),
        ],
        vec![
            format!("{{\"path\":\"{}", "x".repeat(9000)),
            "sec\\u0072et\"}".into(),
        ],
        vec![
            "{\"path\":\"\\uD83D".into(),
            "\\uDD12sec\\u00".into(),
            "72et\"}".into(),
        ],
    ] {
        let body = tool_argument_sse(&arguments);
        let (events, error) = filtered_response_events(&body, "secret").await;
        assert_eq!(
            error,
            Some(ProviderError::FilterStopped {
                rule: "response-stop".into()
            })
        );
        assert!(!events.contains(&ProviderEvent::Done));
    }
}

#[tokio::test]
async fn filter_response_does_not_join_distinct_json_strings_in_tool_arguments() {
    let body = tool_argument_sse(&["{\"path\":\"sec\",\"other\":\"ret\"}".into()]);
    let (events, error) = filtered_response_events(&body, "secret").await;
    assert_eq!(error, None);
    assert!(events.contains(&ProviderEvent::Done));
}

#[tokio::test]
async fn filter_response_exact_url_split_across_frames_is_flushed_at_eof() {
    for suffix in [".com", ".com/child"] {
        let filter = std::sync::Arc::new(
            crate::provider_filter::ProviderFilter::compile(
                &crate::provider_filter::ProviderFilterConfig {
                    enabled: true,
                    urls: vec!["https://example.com".into()],
                    ..crate::provider_filter::ProviderFilterConfig::default()
                },
            )
            .unwrap(),
        );
        let body = format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            serde_json::json!({"choices":[{"delta":{"content":"https://example"}}]}),
            serde_json::json!({"choices":[{"delta":{"content":suffix},"finish_reason":"stop"}]}),
        );
        let (events, error) = filtered_response_events_with_filter(&body, filter).await;
        if suffix == ".com" {
            assert_eq!(
                error,
                Some(ProviderError::FilterStopped {
                    rule: "url-1".into()
                })
            );
            assert!(!events.contains(&ProviderEvent::Done));
        } else {
            assert_eq!(error, None);
            assert!(events.contains(&ProviderEvent::Done));
        }
    }
}

async fn assert_arming_interrupts_silent_response(status: Option<u16>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = tempfile::tempdir().unwrap();
    let trip_file = root.path().join("trip");
    std::fs::write(&trip_file, "stop").unwrap();
    let idle = std::sync::Arc::new(
        crate::provider_filter::ProviderFilter::compile(
            &crate::provider_filter::ProviderFilterConfig::default(),
        )
        .unwrap(),
    );
    let armed = std::sync::Arc::new(
        crate::provider_filter::ProviderFilter::compile(
            &crate::provider_filter::ProviderFilterConfig {
                enabled: true,
                trip_file: Some(trip_file),
                ..crate::provider_filter::ProviderFilterConfig::default()
            },
        )
        .unwrap(),
    );
    let slot = std::sync::Arc::new(std::sync::RwLock::new(idle));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let (sent, ready) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        if let Some(status) = status {
            stream.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Type: text/event-stream\r\nContent-Length: 1000000\r\n\r\n").as_bytes()).await.unwrap();
            if status == 200 {
                stream
                    .write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"ready\"}}]}\n\n")
                    .await
                    .unwrap();
            }
        }
        let _ = sent.send(());
        let mut byte = [0];
        matches!(
            tokio::time::timeout(Duration::from_secs(3), stream.read(&mut byte)).await,
            Ok(Ok(0)) | Ok(Err(_))
        )
    });
    let mut policy = fast_policy();
    policy.headers_timeout = Duration::from_secs(3);
    policy.first_event_timeout = Duration::from_secs(3);
    policy.no_progress_timeout = Duration::from_secs(3);
    let mut client = test_client(url, policy);
    client.filter_override = Some(slot.clone());
    let mut pending = Some(tokio::spawn(
        async move { client.chat_stream(&[], &[]).await },
    ));
    ready.await.unwrap();
    let mut receiver = if status == Some(200) {
        let mut receiver = pending.take().unwrap().await.unwrap().unwrap();
        assert_eq!(
            receiver.recv().await.unwrap().unwrap(),
            ProviderEvent::Text("ready".into())
        );
        Some(receiver)
    } else {
        None
    };
    *slot.write().unwrap() = armed.clone();
    let error = if let Some(receiver) = receiver.as_mut() {
        tokio::time::timeout(Duration::from_millis(700), receiver.recv())
            .await
            .expect("arming must stop a silent body")
            .unwrap()
            .unwrap_err()
    } else {
        tokio::time::timeout(Duration::from_millis(700), pending.take().unwrap())
            .await
            .expect("arming must stop pending headers and error bodies")
            .unwrap()
            .unwrap_err()
    };
    assert!(matches!(error, ProviderError::FilterStopped { .. }));
    assert!(armed.is_tripped());
    assert!(
        server.await.unwrap(),
        "stopping must close the provider socket"
    );
}

#[tokio::test]
async fn filter_arming_interrupts_in_flight_headers() {
    assert_arming_interrupts_silent_response(None).await;
}

#[tokio::test]
async fn filter_arming_interrupts_in_flight_stream() {
    assert_arming_interrupts_silent_response(Some(200)).await;
}

#[tokio::test]
async fn filter_arming_interrupts_in_flight_error_body() {
    assert_arming_interrupts_silent_response(Some(400)).await;
}

#[tokio::test]
async fn filter_arming_checks_the_next_in_flight_response_frame() {
    use tokio::io::AsyncWriteExt;
    let idle = std::sync::Arc::new(
        crate::provider_filter::ProviderFilter::compile(
            &crate::provider_filter::ProviderFilterConfig::default(),
        )
        .unwrap(),
    );
    let slot = std::sync::Arc::new(std::sync::RwLock::new(idle));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let (release, released) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 1000000\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"ready\"}}]}\n\n").await.unwrap();
        released.await.unwrap();
        stream.write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"secret\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n").await.unwrap();
    });
    let mut client = test_client(url, fast_policy());
    client.filter_override = Some(slot.clone());
    let mut receiver = client.chat_stream(&[], &[]).await.unwrap();
    assert_eq!(
        receiver.recv().await.unwrap().unwrap(),
        ProviderEvent::Text("ready".into())
    );
    *slot.write().unwrap() = response_filter("secret");
    release.send(()).unwrap();
    assert_eq!(
        receiver.recv().await.unwrap().unwrap_err(),
        ProviderError::FilterStopped {
            rule: "response-stop".into()
        }
    );
    server.await.unwrap();
}

#[tokio::test]
async fn transport_regression_stalled_503_retries_two_identical_posts() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        let (mut first, _) = listener.accept().await.unwrap();
        requests.push(read_http_request(&mut first).await);
        first
            .write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 1000000\r\n\r\n{")
            .await
            .unwrap();
        let (mut second, _) = listener.accept().await.unwrap();
        requests.push(read_http_request(&mut second).await);
        second
            .write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 1000000\r\n\r\n{")
            .await
            .unwrap();
        let mut byte = [0];
        assert!(matches!(first.read(&mut byte).await, Ok(0) | Err(_)));
        assert!(matches!(second.read(&mut byte).await, Ok(0) | Err(_)));
        assert!(
            tokio::time::timeout(Duration::from_millis(150), listener.accept())
                .await
                .is_err()
        );
        requests
    });
    let mut policy = fast_policy();
    policy.max_attempts = 2;
    policy.headers_timeout = Duration::from_millis(100);
    let error = tokio::time::timeout(
        Duration::from_millis(800),
        test_client(url, policy).chat_stream(&[], &[]),
    )
    .await
    .expect("stalled 503 bodies must not block retry")
    .unwrap_err();
    assert_eq!(error, ProviderError::http(503));
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(request_body(&requests[0]), request_body(&requests[1]));
}

#[tokio::test]
async fn transport_regression_receiver_drop_cancels_retry_error_body() {
    let (url, server, ready) = stalled_error_server(503, b"{".to_vec()).await;
    let mut policy = fast_policy();
    policy.headers_timeout = Duration::from_secs(3);
    policy.max_attempts = 2;
    let client = test_client(url.clone(), policy);
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        let mut attempt = 1;
        open_provider_response(
            &client,
            client.protocol.adapter(),
            &format!("{url}/chat/completions"),
            b"{}",
            &crate::provider_replay::ReplayBindings::default(),
            &mut attempt,
            2,
            None,
            Some(&tx),
        )
        .await
    });
    ready.await.unwrap();
    // Let the already-sent headers reach the request task so cancellation is
    // exercised during body collection rather than while awaiting headers.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!task.is_finished());
    drop(rx);
    let response = tokio::time::timeout(Duration::from_millis(300), task)
        .await
        .expect("receiver cancellation must interrupt retry error-body collection")
        .unwrap()
        .unwrap();
    assert!(response.is_none());
    assert!(
        server.await.unwrap(),
        "cancelled body must close its socket"
    );
}

#[tokio::test]
async fn http_failure_captures_json_error_message_without_raw_body() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        let body = br#"{"error":{"type":"invalid_request_error","message":"model context window exceeded"}}"#;
        stream
            .write_all(
                format!(
                    "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream.write_all(body).await.unwrap();
    });
    let client = test_client(url, fast_policy());
    let error = client.chat_stream(&[], &[]).await.unwrap_err();
    match &error {
        ProviderError::Http {
            status: 400,
            body_excerpt,
        } => {
            assert_eq!(
                body_excerpt.as_deref(),
                Some("model context window exceeded")
            );
        }
        other => panic!("expected HTTP 400, got {other:?}"),
    }
    let visible = error.to_string();
    assert!(visible.contains("model context window exceeded"));
    assert!(!visible.contains("invalid_request_error"));
    server.await.unwrap();
}

#[tokio::test]
async fn retry_after_is_capped_and_retry_stops_after_successful_headers() {
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for attempt in 0..2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(read_http_request(&mut stream).await);
            if attempt == 0 {
                stream
                    .write_all(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 999\r\nContent-Length: 0\r\n\r\n")
                    .await
                    .unwrap();
            } else {
                let body = b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
                stream.write_all(body).await.unwrap();
            }
        }
        requests
    });
    let mut policy = fast_policy();
    policy.max_attempts = 2;
    let started = std::time::Instant::now();
    let mut receiver = test_client(url, policy)
        .chat_stream(&[], &[])
        .await
        .unwrap();
    let turn = crate::agent::stream::consume_stream(&mut receiver, |_| {})
        .await
        .unwrap();
    assert_eq!(turn.finish_reason, FinishReason::Stop);
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(request_body(&requests[0]), request_body(&requests[1]));
}

#[tokio::test]
async fn pre_output_body_transport_failure_retries_once_with_identical_request() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();

        let (mut first, _) = listener.accept().await.unwrap();
        requests.push(read_http_request(&mut first).await);
        first
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 64\r\n\r\n",
            )
            .await
            .unwrap();
        drop(first);

        let (mut second, _) =
            tokio::time::timeout(std::time::Duration::from_millis(500), listener.accept())
                .await
                .expect("pre-output body failure should open one replay")
                .unwrap();
        requests.push(read_http_request(&mut second).await);
        let body = b"data: {\"choices\":[{\"delta\":{\"content\":\"recovered\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        second
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        second.write_all(body).await.unwrap();
        requests
    });
    let mut policy = fast_policy();
    policy.max_attempts = 2;
    let client = test_client(url, policy);
    let mut receiver = client.chat_stream(&[], &[]).await.unwrap();

    let turn = crate::agent::stream::consume_stream(&mut receiver, |_| {})
        .await
        .unwrap();

    assert_eq!(turn.content, "recovered");
    assert_eq!(turn.finish_reason, FinishReason::Stop);
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    assert_eq!(client.request_store.snapshot().unwrap().sequence, 1);
}

#[tokio::test]
async fn exhausted_pre_output_body_recovery_sends_two_identical_requests() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for _ in 0..2 {
            let (mut stream, _) =
                tokio::time::timeout(std::time::Duration::from_millis(500), listener.accept())
                    .await
                    .expect("the shared attempt budget should allow exactly two requests")
                    .unwrap();
            requests.push(read_http_request(&mut stream).await);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 64\r\n\r\n",
                )
                .await
                .unwrap();
        }
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
                .await
                .is_err(),
            "recovery must stop after two physical requests"
        );
        requests
    });
    let mut policy = fast_policy();
    policy.max_attempts = 2;
    let mut receiver = test_client(url, policy)
        .chat_stream(&[], &[])
        .await
        .unwrap();

    assert_eq!(
        receiver.recv().await.unwrap().unwrap_err(),
        ProviderError::Transport(ProviderTransportFailure::Request)
    );
    assert!(receiver.recv().await.is_none(), "one terminal error only");
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
}

#[tokio::test]
async fn header_retry_and_body_failure_share_the_two_request_limit() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for response in [
            b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 0\r\nContent-Length: 0\r\n\r\n"
                .as_slice(),
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 64\r\n\r\n"
                .as_slice(),
        ] {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(read_http_request(&mut stream).await);
            stream.write_all(response).await.unwrap();
        }
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept(),)
                .await
                .is_err()
        );
        requests
    });
    let mut policy = fast_policy();
    policy.max_attempts = 2;
    let mut receiver = test_client(url, policy)
        .chat_stream(&[], &[])
        .await
        .unwrap();

    assert_eq!(
        receiver.recv().await.unwrap().unwrap_err(),
        ProviderError::Transport(ProviderTransportFailure::Request)
    );
    assert!(receiver.recv().await.is_none());
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
}

#[tokio::test]
async fn replay_discards_attempt_local_metadata_before_output() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut first, _) = listener.accept().await.unwrap();
        read_http_request(&mut first).await;
        first
            .write_all(concat!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 1000\r\n\r\n",
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":99,\"completion_tokens\":0,\"total_tokens\":99}}\n\n"
            ).as_bytes())
            .await
            .unwrap();
        drop(first);
        let (mut second, _) = listener.accept().await.unwrap();
        read_http_request(&mut second).await;
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"recovered\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n",
            "data: [DONE]\n\n"
        );
        second
            .write_all(format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ).as_bytes())
            .await
            .unwrap();
    });
    let mut policy = fast_policy();
    policy.max_attempts = 2;
    let mut receiver = test_client(url, policy)
        .chat_stream(&[], &[])
        .await
        .unwrap();
    let mut delivered_usage = Vec::new();
    let turn = crate::agent::stream::consume_stream(&mut receiver, |event| match event {
        ProviderEvent::Usage(usage)
        | ProviderEvent::Finish {
            usage: Some(usage), ..
        } => {
            delivered_usage.push(usage.prompt_tokens);
        }
        _ => {}
    })
    .await
    .unwrap();

    assert_eq!(turn.content, "recovered");
    assert_eq!(turn.usage.unwrap().prompt_tokens, 1);
    assert!(!delivered_usage.contains(&99));
    server.await.unwrap();
}

#[tokio::test]
async fn malformed_pre_output_stream_is_not_replayed() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        let body = "data: not-json\n\n";
        stream
            .write_all(format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ).as_bytes())
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    });
    let mut policy = fast_policy();
    policy.max_attempts = 2;
    let mut receiver = test_client(url, policy)
        .chat_stream(&[], &[])
        .await
        .unwrap();

    assert!(matches!(
        receiver.recv().await.unwrap(),
        Err(ProviderError::MalformedData(_))
    ));
    assert!(server.await.unwrap());
}

#[tokio::test]
async fn http_400_is_not_replayed_by_two_attempt_policy() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        let replayed =
            tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
                .await
                .is_ok();
        (request, replayed)
    });
    let mut policy = fast_policy();
    policy.max_attempts = 2;

    assert_eq!(
        test_client(url, policy)
            .chat_stream(&[], &[])
            .await
            .unwrap_err(),
        ProviderError::Http {
            status: 400,
            body_excerpt: None
        }
    );
    let (request, replayed) = server.await.unwrap();
    assert!(!request_body(&request).is_empty());
    assert!(!replayed, "HTTP 400 must produce exactly one request");
}

async fn assert_pre_output_commit_event_suppresses_replay(
    event_body: &'static [u8],
    options: ChatRequestOptions,
    expected: ProviderEvent,
) {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        // Send headers and the committed event together, so the deliberately
        // short post-event deadline does not measure TCP delayed acknowledgments.
        let mut response =
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 1000\r\n\r\n"
                .to_vec();
        response.extend_from_slice(event_body);
        stream.write_all(&response).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    });
    let mut policy = fast_policy();
    policy.connect_timeout = Duration::from_secs(5);
    policy.headers_timeout = Duration::from_secs(5);
    policy.max_attempts = 2;
    let mut receiver = test_client(url, policy)
        .chat_stream_with_options(&[], &[], &options)
        .await
        .unwrap();

    assert_eq!(receiver.recv().await.unwrap().unwrap(), expected);
    while let Some(item) = receiver.recv().await {
        if let Err(error) = item {
            assert_eq!(
                error,
                ProviderError::Transport(ProviderTransportFailure::Timeout)
            );
            break;
        }
    }
    assert!(
        server.await.unwrap(),
        "usable output must suppress physical replay"
    );
}

#[tokio::test]
async fn reasoning_tool_start_and_tool_arguments_commit_the_body_attempt() {
    assert_pre_output_commit_event_suppresses_replay(
        b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking\"},\"finish_reason\":null}]}\n\n",
        ChatRequestOptions {
            reasoning_stream_field: Some("reasoning_content".into()),
            ..ChatRequestOptions::default()
        },
        ProviderEvent::Reasoning("thinking".into()),
    )
    .await;
    assert_pre_output_commit_event_suppresses_replay(
        b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"name\":\"read_file\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
        ChatRequestOptions::default(),
        ProviderEvent::ToolCallStart {
            index: 0,
            id: "call-1".into(),
            name: "read_file".into(),
        },
    )
    .await;
    assert_pre_output_commit_event_suppresses_replay(
        b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
        ChatRequestOptions::default(),
        ProviderEvent::ToolCallArg {
            index: 0,
            arg: "{}".into(),
        },
    )
    .await;
}

#[tokio::test]
async fn buffered_text_prefix_commits_before_normalization_and_suppresses_replay() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        stream
            .write_all(concat!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 1000\r\n\r\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"<|DSML|\"},\"finish_reason\":null}]}\n\n"
            ).as_bytes())
            .await
            .unwrap();
        drop(stream);
        tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    });
    let mut policy = fast_policy();
    policy.max_attempts = 2;
    let mut receiver = test_client(url, policy)
        .chat_stream(&[], &[])
        .await
        .unwrap();

    assert_eq!(
        receiver.recv().await.unwrap().unwrap_err(),
        ProviderError::Transport(ProviderTransportFailure::Request)
    );
    assert!(server.await.unwrap());
}

#[tokio::test]
async fn dropping_receiver_cancels_pre_output_retry_delay() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 64\r\n\r\n",
            )
            .await
            .unwrap();
        drop(stream);
        tokio::time::timeout(std::time::Duration::from_millis(350), listener.accept())
            .await
            .is_err()
    });
    let mut policy = fast_policy();
    policy.max_attempts = 2;
    policy.retry_delay = std::time::Duration::from_millis(250);
    let receiver = test_client(url, policy)
        .chat_stream(&[], &[])
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;

    drop(receiver);

    assert!(
        server.await.unwrap(),
        "receiver cancellation must stop a pending replay"
    );
}

#[tokio::test]
async fn dropping_receiver_cancels_replay_while_waiting_for_headers() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut first, _) = listener.accept().await.unwrap();
        read_http_request(&mut first).await;
        first
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 64\r\n\r\n",
            )
            .await
            .unwrap();
        drop(first);
        let (mut second, _) = listener.accept().await.unwrap();
        read_http_request(&mut second).await;
        started_tx.send(()).unwrap();
        let mut byte = [0_u8; 1];
        matches!(
            tokio::time::timeout(
                std::time::Duration::from_millis(500),
                second.read(&mut byte),
            )
            .await,
            Ok(Ok(0)) | Ok(Err(_))
        )
    });
    let mut policy = fast_policy();
    policy.max_attempts = 2;
    policy.headers_timeout = std::time::Duration::from_secs(5);
    let receiver = test_client(url, policy)
        .chat_stream(&[], &[])
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_millis(500), started_rx)
        .await
        .unwrap()
        .unwrap();

    drop(receiver);

    assert!(
        server.await.unwrap(),
        "cancellation must close the replay request"
    );
}

#[tokio::test]
async fn one_attempt_policy_sends_exactly_one_physical_post() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    });
    let client = test_client(url, ProviderCallPolicy::one_attempt());

    assert_eq!(
        client.chat_stream(&[], &[]).await.unwrap_err(),
        ProviderError::Http {
            status: 429,
            body_excerpt: None
        }
    );
    assert!(server.await.unwrap(), "one-attempt policy must not replay");
}

#[tokio::test]
async fn responses_options_correlation_header_and_terminal_metadata_are_exact() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        let body = concat!(
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-1\",\"model\":\"gpt-5.6-sol\",\"service_tier\":\"default\"}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-1\",\"model\":\"gpt-5.6-sol\",\"service_tier\":\"default\",\"status\":\"completed\",\"usage\":{\"input_tokens\":10,\"output_tokens\":2,\"total_tokens\":12,\"input_tokens_details\":{\"cached_tokens\":3,\"cache_write_tokens\":4},\"output_tokens_details\":{\"reasoning_tokens\":1}}}}\n\n"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nx-request-id: req-provider-secret\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream.write_all(body.as_bytes()).await.unwrap();
        request
    });
    let client = AgentClient::with_call_policy(
        "openai",
        &ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: url,
            api_key: String::new(),
            protocol: crate::agent::ProviderProtocol::OpenAiResponses,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["gpt-5.6-sol".into()],
            timeout: ProviderTimeoutConfig::default(),
        },
        "gpt-5.6-sol",
        ProviderCallPolicy::one_attempt(),
        crate::request::RequestStore::default(),
    )
    .with_openai_responses_options(OpenAiResponsesRequestOptions::capped(Some(
        OpenAiServiceTier::Default,
    )))
    .unwrap()
    .with_openai_client_request_id("ledger-reservation-1")
    .unwrap();
    let mut receiver = client
        .chat_stream(
            &[ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text("hello"),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            }],
            &[],
        )
        .await
        .unwrap();
    let turn = crate::agent::stream::consume_stream(&mut receiver, |_| {})
        .await
        .unwrap();
    let usage = turn.usage.unwrap();
    assert_eq!(usage.cache_write_tokens, Some(4));
    let metadata = usage.response_metadata.unwrap();
    assert_eq!(metadata.model(), "gpt-5.6-sol");
    assert_eq!(metadata.service_tier(), Some("default"));
    assert_eq!(metadata.request_id_sha256().len(), 64);
    assert!(!format!("{metadata:?}").contains("req-provider-secret"));

    let request = server.await.unwrap();
    assert!(String::from_utf8_lossy(&request)
        .to_ascii_lowercase()
        .contains("x-client-request-id: ledger-reservation-1\r\n"));
    let body: serde_json::Value = serde_json::from_slice(request_body(&request)).unwrap();
    assert_eq!(body["max_output_tokens"], 8_192);
    assert_eq!(body["store"], false);
    assert_eq!(body["service_tier"], "default");
}

#[tokio::test]
async fn partial_stream_times_out_without_opening_a_retry_request() {
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_request(&mut stream).await;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 1000\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n",
            )
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        tokio::time::timeout(std::time::Duration::from_millis(80), listener.accept())
            .await
            .is_err()
    });
    let mut policy = fast_policy();
    policy.max_attempts = 2;
    let mut receiver = test_client(url, policy)
        .chat_stream(&[], &[])
        .await
        .unwrap();
    assert_eq!(
        receiver.recv().await.unwrap().unwrap(),
        ProviderEvent::Text("partial".into())
    );
    assert_eq!(
        receiver.recv().await.unwrap().unwrap_err(),
        ProviderError::Transport(ProviderTransportFailure::Timeout)
    );
    assert!(server.await.unwrap(), "partial stream must not be replayed");
}

#[tokio::test]
async fn provider_trace_captures_retries_frames_events_and_terminal_outcome() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut first, _) = listener.accept().await.unwrap();
        read_http_request(&mut first).await;
        first
            .write_all(
                b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nSet-Cookie: provider-secret\r\nContent-Length: 0\r\n\r\n",
            )
            .await
            .unwrap();
        drop(first);
        let (mut second, _) = listener.accept().await.unwrap();
        read_http_request(&mut second).await;
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok trace-secret\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n",
            "data: [DONE]\n\n"
        );
        second
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nx-request-id: raw-request-id\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        second.write_all(body.as_bytes()).await.unwrap();
    });
    let temp = tempfile::tempdir().unwrap();
    let hub = crate::provider_trace::ProviderTraceHub::new(temp.path().join("traces"), true);
    let crate::provider_trace::TraceStart::Started(trace_id) = hub.start().unwrap() else {
        panic!("first activation must create a trace");
    };
    let mut policy = fast_policy();
    policy.max_attempts = 2;
    let client = AgentClient::with_policy(
        "test",
        &ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: url,
            api_key: "trace-secret".into(),
            protocol: crate::agent::ProviderProtocol::ChatCompletions,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["model".into()],
            timeout: ProviderTimeoutConfig::default(),
        },
        "model",
        policy,
        crate::request::RequestStore::default(),
    )
    .with_trace_hub_for_test(hub.clone());
    let prepared = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::Review,
        "test",
        "model",
        vec![ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text("review this"),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        }],
        Vec::new(),
        ChatRequestOptions::default(),
    );
    let mut receiver = client.stream_prepared(&prepared).await.unwrap();
    crate::agent::stream::consume_stream(&mut receiver, |_| {})
        .await
        .unwrap();
    server.await.unwrap();
    hub.stop_and_flush(Duration::from_secs(1));
    let trace = std::fs::read_to_string(hub.directory().join(format!("{trace_id}.jsonl"))).unwrap();
    assert!(trace.contains("\"request_kind\":\"review\""));
    assert!(trace.contains("\"attempt\":1"));
    assert!(trace.contains("\"attempt\":2"));
    assert!(trace.contains("\"retry\":true"));
    assert!(trace.contains("raw_stream_frame"));
    assert!(trace.contains("decoded_provider_event"));
    assert!(trace.contains("\"outcome\":\"completed\""));
    assert!(trace.contains("[REDACTED]") || trace.contains("\\u005bREDACTED\\u005d"));
    assert!(!trace.contains("trace-secret"));
    assert!(!trace.contains("provider-secret"));
    assert!(!trace.contains("raw-request-id"));
}

#[tokio::test]
async fn provider_trace_captures_all_supported_protocols_at_the_common_boundary() {
    use tokio::io::AsyncWriteExt;

    async fn capture(protocol: crate::agent::ProviderProtocol, body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_http_request(&mut stream).await;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(body.as_bytes()).await.unwrap();
        });
        let temp = tempfile::tempdir().unwrap();
        let hub = crate::provider_trace::ProviderTraceHub::new(temp.path().join("traces"), true);
        let crate::provider_trace::TraceStart::Started(id) = hub.start().unwrap() else {
            panic!("first activation must create a trace");
        };
        let session_id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000117").unwrap();
        // This fixture checks trace contents, not deadline enforcement. Leave
        // room for a loaded verification host while keeping one bounded POST.
        let policy = ProviderCallPolicy {
            connect_timeout: Duration::from_secs(5),
            headers_timeout: Duration::from_secs(5),
            first_event_timeout: Duration::from_secs(5),
            no_progress_timeout: Duration::from_secs(5),
            max_attempts: 1,
            ..fast_policy()
        };
        let client = AgentClient::with_policy(
            "test",
            &ProviderConfig {
                google: None,
                tool_schema_policy: Default::default(),
                base_url: url,
                api_key: String::new(),
                protocol,
                prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                models: vec!["model".into()],
                timeout: ProviderTimeoutConfig::default(),
            },
            "model",
            policy,
            crate::request::RequestStore::default(),
        )
        .with_trace_hub_for_test(hub.clone())
        .with_trace_session_id(session_id);
        let mut receiver = client.chat_stream(&[], &[]).await.unwrap();
        crate::agent::stream::consume_stream(&mut receiver, |_| {})
            .await
            .unwrap();
        server.await.unwrap();
        let path = hub.directory().join(format!("{id}.jsonl"));
        // Stream closure queues the terminal record; flush the writer before
        // reading instead of returning a snapshot captured before the barrier.
        assert_eq!(hub.stop_and_flush(Duration::from_secs(1)), Some(id));
        std::fs::read_to_string(path).unwrap()
    }

    let chat = capture(
        crate::agent::ProviderProtocol::ChatCompletions,
        concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        ),
    )
    .await;
    let responses = capture(
        crate::agent::ProviderProtocol::OpenAiResponses,
        concat!(
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"content_index\":0,\"delta\":\"ok\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n"
        ),
    )
    .await;
    let anthropic = capture(
        crate::agent::ProviderProtocol::AnthropicMessages,
        concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
        ),
    )
    .await;

    for (trace, protocol) in [
        (chat, "chat_completions"),
        (responses, "openai_responses"),
        (anthropic, "anthropic_messages"),
    ] {
        assert!(trace.contains(&format!("\"protocol\":\"{protocol}\"")));
        assert!(trace.contains("attempt_started"));
        assert!(trace.contains("raw_stream_frame"));
        assert!(trace.contains("decoded_provider_event"));
        assert!(trace.contains("\"outcome\":\"completed\""));
        assert!(trace.contains("\"session_id\":\"00000000-0000-0000-0000-000000000117\""));
    }
}

#[test]
fn transient_tool_images_are_inserted_after_the_matching_tool_result() {
    let image = crate::image_content::normalize_rgba(1, 1, vec![1, 1, 1, 255]).unwrap();
    let mut assembly = crate::request::PromptAssembly {
        messages: vec![ChatMessage {
            role: "tool".into(),
            content: crate::image_content::ModelContent::text(image.safe_marker()),
            tool_calls: None,
            tool_call_id: Some("call-image".into()),
            response_items: Vec::new(),
        }],
        stable_prefix_messages: 0,
        segments: vec![crate::request::PromptSegment {
            lifetime: crate::request::PromptSegmentLifetime::AppendOnly,
            messages: 0..1,
        }],
    };
    let images = std::collections::HashMap::from([("call-image".into(), image.clone())]);
    let inserted = append_transient_tool_images(&mut assembly.messages, &images);
    assembly.mark_inserted_messages(
        &inserted,
        crate::request::PromptSegmentLifetime::RequestOnly,
    );

    assert_eq!(inserted, [1]);
    assert_eq!(assembly.messages.len(), 2);
    assert_eq!(assembly.messages[0].role, "tool");
    assert_eq!(assembly.messages[1].role, "user");
    assert!(assembly.messages[1].content.has_images());
    assert_eq!(
        assembly.segments,
        [
            crate::request::PromptSegment {
                lifetime: crate::request::PromptSegmentLifetime::AppendOnly,
                messages: 0..1,
            },
            crate::request::PromptSegment {
                lifetime: crate::request::PromptSegmentLifetime::RequestOnly,
                messages: 1..2,
            },
        ]
    );
    let safe = serde_json::to_string(&assembly.messages[1].content).unwrap();
    assert!(safe.contains("binary content omitted"));
    assert!(!safe.contains(&image.base64()));
}

#[test]
fn hrs253_output_cap_applies_to_every_protocol_without_relaxing_adapter_limits() {
    use super::apply_output_token_cap;
    use crate::agent::provider::ProviderProtocol;
    for (protocol, direct, field) in [
        (ProviderProtocol::ChatCompletions, false, "max_tokens"),
        (
            ProviderProtocol::ChatCompletions,
            true,
            "max_completion_tokens",
        ),
        (ProviderProtocol::OpenAiResponses, true, "max_output_tokens"),
        (ProviderProtocol::AnthropicMessages, false, "max_tokens"),
    ] {
        let mut body = br#"{"model":"test","max_tokens":4000}"#.to_vec();
        apply_output_token_cap(&mut body, protocol, direct, 1000).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value[field], 1000);
        assert_eq!(value.as_object().unwrap().len(), 2);
        apply_output_token_cap(&mut body, protocol, direct, 8000).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value[field], 1000);
    }
}

#[tokio::test]
async fn hrs253_input_body_cap_rejects_every_protocol_before_transport() {
    use crate::agent::provider::ProviderProtocol;
    for protocol in [
        ProviderProtocol::ChatCompletions,
        ProviderProtocol::OpenAiResponses,
        ProviderProtocol::AnthropicMessages,
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider = ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
            api_key: "synthetic-test-token".into(),
            protocol,
            prompt_cache_mode: Default::default(),
            models: vec!["test".into()],
            timeout: Default::default(),
        };
        let client = AgentClient::with_call_policy(
            "fake",
            &provider,
            "test",
            ProviderCallPolicy::one_attempt(),
            crate::request::RequestStore::default(),
        )
        .with_input_byte_cap(512)
        .unwrap();
        let message = ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text("é".repeat(300)),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        };
        let error = client.chat_stream(&[message], &[]).await.unwrap_err();
        assert!(
            error.to_string().contains("input byte cap"),
            "{protocol:?}: {error}"
        );
        let tool = ToolDef {
            def_type: "function".into(),
            function: ToolFunction {
                name: "test".into(),
                description: "schema".repeat(200),
                parameters: serde_json::json!({"type": "object"}),
            },
        };
        let error = client.chat_stream(&[], &[tool]).await.unwrap_err();
        assert!(
            error.to_string().contains("input byte cap"),
            "{protocol:?}: {error}"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(30), listener.accept())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn hrs253_prepared_requests_expose_the_actual_route_before_outcome_capture() {
    let provider = ProviderConfig {
        google: None,
        tool_schema_policy: Default::default(),
        base_url: "http://127.0.0.1:1/v1".into(),
        api_key: "synthetic".into(),
        protocol: crate::agent::provider::ProviderProtocol::ChatCompletions,
        prompt_cache_mode: Default::default(),
        models: vec!["test".into()],
        timeout: Default::default(),
    };
    let client = AgentClient::with_request_store(
        "fake",
        &provider,
        "test",
        crate::request::RequestStore::default(),
    );
    let request = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::Compatibility,
        "fake",
        "test",
        Vec::new(),
        Vec::new(),
        ChatRequestOptions::default(),
    );
    let unresolved = crate::harness_registry::capture_prepared(&request).outcome_snapshot();
    let prepared = client.prepare_harness_request(request).await;
    let actual = crate::harness_registry::capture_prepared(&prepared).outcome_snapshot();
    let explicit = crate::harness_registry::capture_prepared_on_route(
        &prepared,
        &client.route_identity().unwrap(),
    )
    .outcome_snapshot();
    assert_eq!(actual, explicit);
    assert_ne!(unresolved.model_route, actual.model_route);
}

#[tokio::test]
async fn orc290_selective_pruning_crosses_all_three_actual_provider_transports() {
    for protocol in [
        crate::agent::ProviderProtocol::ChatCompletions,
        crate::agent::ProviderProtocol::OpenAiResponses,
        crate::agent::ProviderProtocol::AnthropicMessages,
    ] {
        let mut canonical = vec![];
        for i in 0..5 {
            canonical.push(user(&format!("turn{i}")));
            let mut tool = call(
                &format!("p{i}"),
                ToolStatus::Done,
                Some(&"PRUNABLE_EVIDENCE ".repeat(2500)),
            );
            tool.arguments = "{\"path\":\"evidence.txt\"}".into();
            canonical.push(assistant_with_calls(vec![tool]));
        }
        let before = serde_json::to_vec(&canonical).unwrap();
        let mut messages = AgentClient::build_messages(None, &canonical, None, &[]).unwrap();
        let stats = crate::context_pruning::project(&mut messages, true, Some(8000));
        assert!(stats.results > 0);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let (mut socket, _) = listener.accept().await.unwrap();
            let bytes = read_http_request(&mut socket).await;
            socket
                .write_all(
                    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                )
                .await
                .unwrap();
            serde_json::from_slice::<serde_json::Value>(request_body(&bytes)).unwrap()
        });
        let provider = ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: format!("http://{address}/v1"),
            api_key: String::new(),
            protocol,
            prompt_cache_mode: crate::config::PromptCacheMode::Off,
            models: vec!["fixture".into()],
            timeout: ProviderTimeoutConfig::default(),
        };
        let client = AgentClient::with_call_policy(
            "fixture",
            &provider,
            "fixture",
            ProviderCallPolicy::one_attempt(),
            Default::default(),
        );
        let request = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::MainTurn,
            "fixture",
            "fixture",
            messages,
            vec![],
            Default::default(),
        );
        let _expected_fixture_rejection = client.stream_prepared(&request).await;
        let body = server.await.unwrap();
        let encoded = serde_json::to_string(&body).unwrap();
        assert!(encoded.contains("omitted from this request"));
        assert!(encoded.contains("turn4"));
        assert!(encoded.contains("p0"));
        assert_eq!(serde_json::to_vec(&canonical).unwrap(), before);
    }
}

#[tokio::test]
async fn google_strict_schema_rejection_is_visible_before_any_post_without_schema_values() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let hub = crate::provider_trace::ProviderTraceHub::new(temp.path().join("traces"), true);
    let crate::provider_trace::TraceStart::Started(id) = hub.start().unwrap() else {
        panic!()
    };
    let store = crate::request::RequestStore::default();
    let client = AgentClient::with_policy(
        "google-fixture",
        &ProviderConfig {
            google: None,
            tool_schema_policy: crate::tool_schema::Policy::RejectLossy,
            base_url: format!("http://{address}/v1beta"),
            api_key: "private-google-key".into(),
            protocol: crate::agent::ProviderProtocol::GoogleGenerateContent,
            prompt_cache_mode: Default::default(),
            models: vec!["fixture-model".into()],
            timeout: Default::default(),
        },
        "fixture-model",
        fast_policy(),
        store.clone(),
    )
    .with_trace_hub_for_test(hub.clone());
    let tools = vec![ToolDef {
        def_type: "function".into(),
        function: ToolFunction {
            name: "example".into(),
            description: "private-schema-description".into(),
            parameters: serde_json::json!({"type":"object","additionalProperties":false,"properties":{"value":{"type":"string","enum":["private-schema-value"]}}}),
        },
    }];
    let request = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::MainTurn,
        "google-fixture",
        "fixture-model",
        vec![],
        tools,
        ChatRequestOptions::default(),
    );
    let error = client.stream_prepared(&request).await.unwrap_err();
    assert!(error.to_string().contains("reject_lossy"));
    assert!(
        tokio::time::timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err()
    );
    let snapshot = store.snapshot().unwrap();
    assert!(snapshot.render_text().contains("Rejected tool example"));
    assert!(snapshot
        .adapter_diagnostics
        .iter()
        .any(|diagnostic| diagnostic.contains("constraint_dropped")
            && diagnostic.contains("/additionalProperties")));
    hub.stop_and_flush(Duration::from_secs(1));
    let trace = std::fs::read_to_string(hub.directory().join(format!("{id}.jsonl"))).unwrap();
    assert!(trace.contains("adapter_diagnostics"));
    assert!(trace.contains("blocked_tool_schema"));
    assert!(!trace.contains("attempt_started"));
    for private in [
        "private-schema-description",
        "private-schema-value",
        "private-google-key",
    ] {
        assert!(!trace.contains(private));
    }
    assert!(
        crate::google_provider::rejected_schema_diagnostics(&ProviderError::Protocol(
            "unrelated provider error".into()
        ))
        .is_none()
    );
}
