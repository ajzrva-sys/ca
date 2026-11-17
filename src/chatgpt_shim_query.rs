//! One-question queries from another agent to the local ChatGPT chat shim.
//! The shim receives the question only: no task map, no tool protocol.

use crate::config::{CaConfig, ProviderConfig};

pub const PROVIDER_ID: &str = "chatgpt-shim";
const DEFAULT_BASE: &str = "http://127.0.0.1:8377/v1";
const DEFAULT_MODEL: &str = "gpt-5.6-thinking";
const MAX_QUESTION_CHARS: usize = 16 * 1024;
const MAX_ANSWER_BYTES: usize = 24 * 1024;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShimEndpoint {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

pub fn endpoint_for(
    config: &CaConfig,
    model_override: Option<&str>,
) -> Result<ShimEndpoint, String> {
    let provider = config.providers.get(PROVIDER_ID).ok_or_else(|| {
        "ChatGPT shim is not configured. Add a chatgpt-shim provider before asking it.".to_string()
    })?;
    let model = chosen_model(provider, config, model_override)?;
    let base_url = provider.base_url.trim().trim_end_matches('/').to_string();
    if base_url.is_empty() {
        return Err("ChatGPT shim provider has no base URL.".into());
    }
    Ok(ShimEndpoint {
        base_url,
        api_key: provider.api_key.clone(),
        model,
    })
}

fn chosen_model(
    provider: &ProviderConfig,
    config: &CaConfig,
    model_override: Option<&str>,
) -> Result<String, String> {
    if let Some(model) = model_override
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        validate_model(model)?;
        return Ok(model.to_string());
    }
    if config.defaults.provider == PROVIDER_ID {
        let model = config.defaults.model.trim();
        if !model.is_empty() {
            validate_model(model)?;
            return Ok(model.to_string());
        }
    }
    provider
        .models
        .iter()
        .find(|model| !model.trim().is_empty())
        .map(|model| model.trim().to_string())
        .or_else(|| Some(DEFAULT_MODEL.to_string()))
        .ok_or_else(|| "ChatGPT shim has no model.".to_string())
}

fn validate_model(model: &str) -> Result<(), String> {
    if model.len() > 64
        || !model
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err("ChatGPT model id must be a short name such as gpt-5.6-thinking.".into());
    }
    Ok(())
}

pub async fn ask(endpoint: &ShimEndpoint, question: &str) -> Result<String, String> {
    let question = question.trim();
    if question.is_empty() {
        return Err("Ask ChatGPT with a non-empty question.".into());
    }
    if question.chars().count() > MAX_QUESTION_CHARS {
        return Err("That question is too long for one ChatGPT query.".into());
    }
    let url = format!("{}/chat/completions", endpoint.base_url);
    let body = serde_json::json!({
        "model": endpoint.model,
        "stream": true,
        "messages": [{ "role": "user", "content": question }],
    });
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|_| "ChatGPT query client could not start.".to_string())?;
    let mut request = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&body);
    if !endpoint.api_key.is_empty() {
        request = request.bearer_auth(&endpoint.api_key);
    }
    let response = request.send().await.map_err(|_| {
        format!(
            "ChatGPT shim is not reachable at {}. Start the shim and retry.",
            endpoint.base_url
        )
    })?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|_| "ChatGPT shim closed the answer before it finished.".to_string())?;
    if !status.is_success() {
        let excerpt = String::from_utf8_lossy(&bytes);
        let excerpt = excerpt.chars().take(240).collect::<String>();
        return Err(format!("ChatGPT shim returned HTTP {status}: {excerpt}"));
    }
    let answer = answer_from_sse(&String::from_utf8_lossy(&bytes))?;
    if answer.trim().is_empty() {
        return Err("ChatGPT shim returned an empty answer.".into());
    }
    Ok(truncate_answer(&answer))
}

fn answer_from_sse(body: &str) -> Result<String, String> {
    let mut answer = String::new();
    for line in body.lines() {
        let Some(data) = line.trim().strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(data)
            .map_err(|_| "ChatGPT shim sent an unreadable answer.".to_string())?;
        if let Some(message) = value
            .pointer("/error/message")
            .and_then(|message| message.as_str())
        {
            return Err(format!("ChatGPT shim error: {message}"));
        }
        if let Some(text) = value
            .pointer("/choices/0/delta/content")
            .and_then(|text| text.as_str())
            .or_else(|| {
                value
                    .pointer("/choices/0/message/content")
                    .and_then(|text| text.as_str())
            })
        {
            if text.contains("[chatgpt-shim upstream error") {
                return Err(text.chars().take(300).collect());
            }
            answer.push_str(text);
        }
    }
    Ok(answer)
}

fn truncate_answer(answer: &str) -> String {
    if answer.len() <= MAX_ANSWER_BYTES {
        return answer.to_string();
    }
    let mut end = MAX_ANSWER_BYTES;
    while !answer.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = answer[..end].to_string();
    out.push_str("\n[truncated]");
    out
}

pub async fn ask_for_agent(question: &str, model: Option<&str>) -> Result<String, String> {
    let endpoint = match crate::config::CaConfig::load() {
        Ok(config) if config.providers.contains_key(PROVIDER_ID) => endpoint_for(&config, model)?,
        Ok(_) | Err(_) => fallback_endpoint(model)?,
    };
    ask(&endpoint, question).await
}

pub fn fallback_endpoint(model_override: Option<&str>) -> Result<ShimEndpoint, String> {
    if let Some(model) = model_override
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        validate_model(model)?;
        return Ok(ShimEndpoint {
            base_url: DEFAULT_BASE.trim_end_matches('/').to_string(),
            api_key: String::new(),
            model: model.to_string(),
        });
    }
    Ok(ShimEndpoint {
        base_url: DEFAULT_BASE.trim_end_matches('/').to_string(),
        api_key: String::new(),
        model: DEFAULT_MODEL.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(base: &str, models: &[&str]) -> CaConfig {
        let mut config = CaConfig::default_config();
        config.providers.insert(
            PROVIDER_ID.to_string(),
            ProviderConfig {
                google: None,
                tool_schema_policy: Default::default(),
                base_url: base.into(),
                api_key: "local-key".into(),
                protocol: crate::agent::ProviderProtocol::ChatCompletions,
                prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                models: models.iter().map(|model| (*model).to_string()).collect(),
                timeout: crate::config::ProviderTimeoutConfig::default(),
            },
        );
        config.defaults.provider = PROVIDER_ID.into();
        config.defaults.model = "gpt-5.6-thinking".into();
        config
    }

    #[test]
    fn upstream_error_text_is_not_an_answer() {
        let error = answer_from_sse(
            "data: {\"choices\":[{\"delta\":{\"content\":\"[chatgpt-shim upstream error 401: prepare]\"}}]}\n\n",
        )
        .unwrap_err();
        assert!(error.contains("[chatgpt-shim upstream error"));
        let reported = answer_from_sse(
            "data: {\"error\":{\"message\":\"chatgpt.com is still on the Cloudflare interstitial\"}}\n\n",
        )
        .unwrap_err();
        assert!(reported.contains("Cloudflare interstitial"));
    }

    #[test]
    fn endpoint_uses_the_configured_shim_and_an_override_model() {
        let config = provider("http://127.0.0.1:8377/v1/", &["gpt-5.6"]);
        let endpoint = endpoint_for(&config, Some("research")).unwrap();
        assert_eq!(endpoint.base_url, "http://127.0.0.1:8377/v1");
        assert_eq!(endpoint.model, "research");
        assert_eq!(endpoint.api_key, "local-key");
        let configured = endpoint_for(&config, None).unwrap();
        assert_eq!(configured.model, "gpt-5.6-thinking");
    }

    #[tokio::test]
    async fn ask_sends_only_the_question_and_reads_the_streamed_answer() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0_u8; 2048];
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let count = tokio::time::timeout_at(
                    deadline,
                    tokio::io::AsyncReadExt::read(&mut socket, &mut chunk),
                )
                .await
                .expect("request headers")
                .unwrap();
                if count == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..count]);
                if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let header_end = bytes
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .unwrap()
                + 4;
            let headers = String::from_utf8_lossy(&bytes[..header_end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            while bytes.len() < header_end + length {
                let count = tokio::time::timeout_at(
                    deadline,
                    tokio::io::AsyncReadExt::read(&mut socket, &mut chunk),
                )
                .await
                .expect("request body")
                .unwrap();
                bytes.extend_from_slice(&chunk[..count]);
            }
            let answer = concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"ChatGPT says \"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"hello.\"},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                answer.len()
            );
            tokio::io::AsyncWriteExt::write_all(&mut socket, response.as_bytes())
                .await
                .unwrap();
            bytes
        });
        let endpoint = ShimEndpoint {
            base_url: format!("http://{address}/v1"),
            api_key: "local-key".into(),
            model: "gpt-5.6-thinking".into(),
        };
        let answer = ask(&endpoint, "What is the capital of France?")
            .await
            .unwrap();
        assert_eq!(answer, "ChatGPT says hello.");
        let sent = String::from_utf8(server.await.unwrap()).unwrap();
        assert!(sent.contains("What is the capital of France?"), "{sent}");
        assert!(sent.contains("gpt-5.6-thinking"), "{sent}");
        assert!(!sent.contains("\"tools\""), "{sent}");
        assert!(!sent.contains("TOOL_CALL"), "{sent}");
        assert!(!sent.contains("focused-author"), "{sent}");
        let (headers, body) = sent.split_once("\r\n\r\n").unwrap();
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("authorization: bearer local-key"),
            "{headers}"
        );
        assert!(!body.contains("local-key"), "{body}");
    }
}
