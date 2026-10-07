use crate::agent::client::{AgentClient, ChatMessage};
use crate::agent::stream::{
    AssistantTurnAssembler, ProviderError, ProviderEvent, ProviderTransportFailure,
};
use crate::config::ProviderConfig;
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_PROBE_STREAM_BYTES: usize = 64 * 1024;
const PROBE_PROMPT: &str = "Reply with exactly OK and nothing else.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderCheckStatus {
    Checking,
    Working { model: String },
    MissingEnvironment { variable: String },
    CredentialRejected,
    AccessDenied,
    TimedOut,
    CannotConnect,
    EndpointNotFound,
    RateLimited,
    ProviderError { status: Option<u16> },
    InvalidResponse,
    NeedsModel,
    AuthenticationRequired,
}

impl ProviderCheckStatus {
    pub fn working_model(&self) -> Option<&str> {
        match self {
            Self::Working { model } => Some(model),
            _ => None,
        }
    }
}

pub struct ProviderProbeRequest {
    pub provider: String,
    pub model: String,
    pub config: ProviderConfig,
    pub usage: Option<crate::usage::UsageRecorder>,
    pub request_store: crate::request::RequestStore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderProbeResult {
    pub provider: String,
    pub model: String,
    pub status: ProviderCheckStatus,
}

pub async fn probe_provider(request: ProviderProbeRequest) -> ProviderProbeResult {
    probe_provider_with_timeout(request, PROBE_TIMEOUT).await
}

async fn probe_provider_with_timeout(
    request: ProviderProbeRequest,
    timeout: Duration,
) -> ProviderProbeResult {
    let ProviderProbeRequest {
        provider,
        model,
        config,
        usage,
        request_store,
    } = request;
    let config = match crate::provider_connect::resolve_runtime_provider(&provider, config).await {
        Ok(config) => config,
        Err(_) => {
            return ProviderProbeResult {
                provider,
                model,
                status: ProviderCheckStatus::AuthenticationRequired,
            }
        }
    };
    let mut usage_span = usage.map(|recorder| recorder.begin());
    let client = AgentClient::with_request_store(provider.clone(), &config, &model, request_store);
    let messages = [ChatMessage {
        role: "user".into(),
        content: crate::image_content::ModelContent::text(PROBE_PROMPT),
        tool_calls: None,
        tool_call_id: None,
        response_items: Vec::new(),
    }];
    let checked = tokio::time::timeout(timeout, async {
        let prepared = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::ProviderProbe,
            provider.clone(),
            model.clone(),
            messages.to_vec(),
            Vec::new(),
            crate::agent::client::ChatRequestOptions::default(),
        );
        let mut receiver = client.stream_prepared(&prepared).await?;
        let mut assembler = AssistantTurnAssembler::new();
        let mut received_bytes = 0usize;
        while let Some(item) = receiver.recv().await {
            let event = item?;
            received_bytes = received_bytes.saturating_add(provider_event_size(&event));
            if received_bytes > MAX_PROBE_STREAM_BYTES {
                return Err(ProviderError::EventTooLarge {
                    limit: MAX_PROBE_STREAM_BYTES,
                });
            }
            if let (Some(span), ProviderEvent::Usage(usage)) = (usage_span.as_mut(), &event) {
                span.observe(usage);
            }
            assembler.push(&event)?;
        }
        let turn = assembler.finish()?;
        if !turn.tool_calls.is_empty() {
            return Err(ProviderError::Protocol(
                "provider probe returned an unexpected tool call".into(),
            ));
        }
        Ok::<_, ProviderError>(turn.usage)
    })
    .await;

    let status = match checked {
        Err(_) => {
            if let Some(span) = usage_span.take() {
                span.fail(None);
            }
            ProviderCheckStatus::TimedOut
        }
        Ok(Ok(usage)) => {
            if let Some(span) = usage_span.take() {
                span.complete(usage);
            }
            ProviderCheckStatus::Working {
                model: model.clone(),
            }
        }
        Ok(Err(error)) => {
            if let Some(span) = usage_span.take() {
                span.fail(None);
            }
            status_for_error(error)
        }
    };
    ProviderProbeResult {
        provider,
        model,
        status,
    }
}

fn status_for_error(error: ProviderError) -> ProviderCheckStatus {
    match error {
        ProviderError::MissingEnvironment(variable) => {
            ProviderCheckStatus::MissingEnvironment { variable }
        }
        ProviderError::Http { status: 401 } => ProviderCheckStatus::CredentialRejected,
        ProviderError::Http { status: 403 } => ProviderCheckStatus::AccessDenied,
        ProviderError::Http { status: 404 } => ProviderCheckStatus::EndpointNotFound,
        ProviderError::Http { status: 429 } => ProviderCheckStatus::RateLimited,
        ProviderError::Http { status } => ProviderCheckStatus::ProviderError {
            status: Some(status),
        },
        ProviderError::Transport(ProviderTransportFailure::Timeout) => {
            ProviderCheckStatus::TimedOut
        }
        ProviderError::Transport(ProviderTransportFailure::Connect) => {
            ProviderCheckStatus::CannotConnect
        }
        ProviderError::Transport(ProviderTransportFailure::Request) => {
            ProviderCheckStatus::ProviderError { status: None }
        }
        ProviderError::InvalidUtf8
        | ProviderError::EventTooLarge { .. }
        | ProviderError::MalformedData(_)
        | ProviderError::PrematureEof
        | ProviderError::IncompleteFinish(_)
        | ProviderError::ResponseFailed
        | ProviderError::ResponseIncomplete
        | ProviderError::OutputTokenLimit
        | ProviderError::ResponseRefused
        | ProviderError::UnresolvedToolCall { .. }
        | ProviderError::Protocol(_)
        | ProviderError::StreamAnomaly(_) => ProviderCheckStatus::InvalidResponse,
    }
}

fn provider_event_size(event: &ProviderEvent) -> usize {
    match event {
        ProviderEvent::Text(text) | ProviderEvent::Reasoning(text) => text.len(),
        ProviderEvent::ToolCallStart { id, name, .. } => id.len().saturating_add(name.len()),
        ProviderEvent::ToolCallArg { arg, .. } => arg.len(),
        ProviderEvent::ResponseItem(_)
        | ProviderEvent::Finish { .. }
        | ProviderEvent::Usage(_)
        | ProviderEvent::Done => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderTimeoutConfig;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn one_shot_server(
        status: &str,
        body: Vec<u8>,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_string();
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 16 * 1024];
            let length = stream.read(&mut request).await.unwrap();
            request.truncate(length);
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.write_all(&body).await.unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        (format!("http://{address}/v1"), handle)
    }

    fn request(provider: &str, base_url: String, api_key: &str) -> ProviderProbeRequest {
        ProviderProbeRequest {
            provider: provider.into(),
            model: "test-model".into(),
            config: ProviderConfig {
                base_url,
                api_key: api_key.into(),
                protocol: crate::agent::ProviderProtocol::default(),
                prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                models: vec!["test-model".into()],
                timeout: ProviderTimeoutConfig::default(),
            },
            usage: None,
            request_store: crate::request::RequestStore::default(),
        }
    }

    fn complete_stream() -> Vec<u8> {
        concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"discard-me\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":2,\"total_tokens\":11}}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        )
        .as_bytes()
        .to_vec()
    }

    fn responses_complete_stream() -> Vec<u8> {
        concat!(
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"content_index\":0,\"delta\":\"discard-me\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":9,\"output_tokens\":2,\"total_tokens\":11}}}\n\n"
        )
        .as_bytes()
        .to_vec()
    }

    fn anthropic_complete_stream() -> Vec<u8> {
        concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"content\":[],\"usage\":{\"input_tokens\":9,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"discard-me\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
        )
        .as_bytes()
        .to_vec()
    }

    #[tokio::test]
    async fn successful_probe_is_isolated_authenticated_and_discards_output() {
        let (base_url, captured) = one_shot_server("200 OK", complete_stream()).await;
        let ledger =
            std::sync::Arc::new(std::sync::Mutex::new(crate::usage::UsageLedger::default()));
        let mut probe = request("safe", base_url, "sentinel-secret");
        let request_store = probe.request_store.clone();
        probe.usage = Some(crate::usage::UsageRecorder::new(
            ledger.clone(),
            crate::usage::UsageKind::ProviderProbe,
            "safe",
            "test-model",
        ));
        let result = probe_provider(probe).await;

        assert_eq!(
            result.status,
            ProviderCheckStatus::Working {
                model: "test-model".into()
            }
        );
        let request = captured.await.unwrap();
        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer sentinel-secret"));
        assert!(request.contains(PROBE_PROMPT));
        assert!(!request.contains("tools"));
        let debug = format!("{result:?}");
        assert!(!debug.contains("sentinel-secret"));
        assert!(!debug.contains("discard-me"));
        let records = ledger.lock().unwrap().snapshot().records;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, crate::usage::UsageOutcome::Completed);
        assert_eq!(records[0].usage.as_ref().unwrap().prompt_tokens, 9);
        let snapshot = request_store.snapshot().unwrap();
        assert_eq!(snapshot.kind, crate::request::RequestKind::ProviderProbe);
        assert_eq!(snapshot.provider, "safe");
        let rendered = snapshot.render_text();
        assert!(rendered.contains(PROBE_PROMPT));
        assert!(!rendered.contains("sentinel-secret"));
    }

    #[tokio::test]
    async fn credentialless_probe_omits_authorization() {
        let (base_url, captured) = one_shot_server("200 OK", complete_stream()).await;
        let result = probe_provider(request("local", base_url, "")).await;

        assert!(result.status.working_model().is_some());
        assert!(!captured
            .await
            .unwrap()
            .to_ascii_lowercase()
            .contains("authorization:"));
    }

    #[tokio::test]
    async fn openai_responses_probe_uses_selected_protocol_and_stays_isolated() {
        let (base_url, captured) = one_shot_server("200 OK", responses_complete_stream()).await;
        let mut probe = request("responses", base_url, "sentinel-secret");
        probe.config.protocol = crate::agent::ProviderProtocol::OpenAiResponses;
        let result = probe_provider(probe).await;

        assert_eq!(
            result.status,
            ProviderCheckStatus::Working {
                model: "test-model".into()
            }
        );
        let request = captured.await.unwrap();
        assert!(request.starts_with("POST /v1/responses HTTP/1.1"));
        assert!(request.contains(PROBE_PROMPT));
        assert!(!format!("{result:?}").contains("sentinel-secret"));
        assert!(!format!("{result:?}").contains("discard-me"));
    }

    #[tokio::test]
    async fn anthropic_probe_uses_selected_protocol_and_stays_isolated() {
        let (base_url, captured) = one_shot_server("200 OK", anthropic_complete_stream()).await;
        let mut probe = request("anthropic", base_url, "sentinel-secret");
        probe.config.protocol = crate::agent::ProviderProtocol::AnthropicMessages;
        let result = probe_provider(probe).await;

        assert_eq!(
            result.status,
            ProviderCheckStatus::Working {
                model: "test-model".into()
            }
        );
        let request = captured.await.unwrap();
        assert!(request.starts_with("POST /v1/messages HTTP/1.1"));
        assert!(request.contains("x-api-key: sentinel-secret"));
        assert!(request.contains("anthropic-version: 2023-06-01"));
        assert!(request.contains(PROBE_PROMPT));
        assert!(!format!("{result:?}").contains("sentinel-secret"));
        assert!(!format!("{result:?}").contains("discard-me"));
    }

    #[tokio::test]
    async fn rejected_credential_drops_response_body_and_secret() {
        let marker = "response-body-must-not-escape";
        let (base_url, captured) = one_shot_server(
            "401 Unauthorized",
            format!("{{\"error\":\"{marker}\"}}").into_bytes(),
        )
        .await;
        let result = probe_provider(request("bad", base_url, "sentinel-secret")).await;

        assert_eq!(result.status, ProviderCheckStatus::CredentialRejected);
        let debug = format!("{result:?}");
        assert!(!debug.contains(marker));
        assert!(!debug.contains("sentinel-secret"));
        captured.await.unwrap();
    }

    #[tokio::test]
    async fn missing_environment_fails_before_network_io() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        let result = probe_provider(request("missing", base_url, "${MISSING_PROBE_KEY}"));
        let (result, accepted) = tokio::join!(
            result,
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
        );

        assert_eq!(
            result.status,
            ProviderCheckStatus::MissingEnvironment {
                variable: "MISSING_PROBE_KEY".into()
            }
        );
        assert!(accepted.is_err());
    }

    #[tokio::test]
    async fn timeout_and_incomplete_stream_are_not_working() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
        });
        let timed_out = probe_provider_with_timeout(
            request("slow", base_url, "key"),
            Duration::from_millis(50),
        )
        .await;
        assert_eq!(timed_out.status, ProviderCheckStatus::TimedOut);
        server.abort();

        let body = b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n".to_vec();
        let (base_url, captured) = one_shot_server("200 OK", body).await;
        let incomplete = probe_provider(request("partial", base_url, "key")).await;
        assert_eq!(incomplete.status, ProviderCheckStatus::InvalidResponse);
        captured.await.unwrap();
    }

    #[test]
    fn http_and_transport_failures_have_typed_safe_statuses() {
        assert_eq!(
            status_for_error(ProviderError::OutputTokenLimit),
            status_for_error(ProviderError::ResponseIncomplete)
        );
        assert_eq!(
            status_for_error(ProviderError::Http { status: 403 }),
            ProviderCheckStatus::AccessDenied
        );
        assert_eq!(
            status_for_error(ProviderError::Http { status: 404 }),
            ProviderCheckStatus::EndpointNotFound
        );
        assert_eq!(
            status_for_error(ProviderError::Http { status: 429 }),
            ProviderCheckStatus::RateLimited
        );
        assert_eq!(
            status_for_error(ProviderError::Transport(ProviderTransportFailure::Connect)),
            ProviderCheckStatus::CannotConnect
        );
        assert_eq!(
            status_for_error(ProviderError::StreamAnomaly(
                crate::agent::stream::StreamAnomalyKind::RepetitionLoop
            )),
            ProviderCheckStatus::InvalidResponse
        );
    }
}
