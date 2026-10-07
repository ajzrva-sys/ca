use crate::config::ProviderConfig;
use futures::{stream, StreamExt};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

const MAX_CATALOG_BYTES: usize = 1024 * 1024;
const MAX_MODEL_ID_BYTES: usize = 256;
const MAX_MODEL_COUNT: usize = 2048;
const MAX_CONCURRENT_PROVIDERS: usize = 8;
const CATALOG_TIMEOUT: Duration = Duration::from_secs(10);
// Wire compatibility, not CA's version or a dependency on an installed Codex.
const CODEX_CATALOG_CLIENT_VERSION: &str = "0.153.0";

#[derive(Debug, Default)]
pub struct CatalogRefresh {
    pub catalogs: HashMap<String, Vec<String>>,
    pub documented: HashMap<String, Vec<String>>,
    pub failures: Vec<(String, String)>,
}

#[derive(Debug, Deserialize)]
struct ModelListResponse {
    data: Vec<ModelObject>,
}

#[derive(Deserialize)]
struct CodexModelListResponse {
    models: Vec<CodexModelObject>,
}

// Deliberately do not deserialize instructions, API eligibility, or capabilities.
#[derive(Deserialize)]
struct CodexModelObject {
    slug: String,
    #[serde(default)]
    visibility: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelObject {
    id: String,
    #[serde(default)]
    architecture: Option<ModelArchitecture>,
    #[serde(default)]
    supported_parameters: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ModelArchitecture {
    #[serde(default)]
    output_modalities: Vec<String>,
}

impl ModelObject {
    fn is_agent_compatible(&self) -> bool {
        self.architecture.as_ref().is_none_or(|architecture| {
            architecture
                .output_modalities
                .iter()
                .any(|modality| modality == "text")
        }) && self
            .supported_parameters
            .as_ref()
            .is_none_or(|parameters| parameters.iter().any(|parameter| parameter == "tools"))
    }
}

#[derive(Clone, PartialEq, Eq)]
enum CatalogAuthentication {
    Bearer,
    Codex { account_id: Option<String> },
    Anthropic,
    Public,
}

pub async fn discover_provider_models(
    providers: HashMap<String, ProviderConfig>,
) -> CatalogRefresh {
    let documented = providers
        .iter()
        .filter_map(|(name, provider)| {
            let models = documented_chat_models(name, provider);
            (!models.is_empty()).then(|| (name.clone(), models))
        })
        .collect();
    let client = match reqwest::Client::builder().timeout(CATALOG_TIMEOUT).build() {
        Ok(client) => client,
        Err(error) => {
            return CatalogRefresh {
                catalogs: HashMap::new(),
                documented,
                failures: providers
                    .into_keys()
                    .map(|provider| (provider, format!("client setup failed: {error}")))
                    .collect(),
            };
        }
    };
    let requests = stream::iter(providers).map(|(name, provider)| {
        let client = client.clone();
        async move {
            let provider =
                match crate::provider_connect::resolve_runtime_provider(&name, provider).await {
                    Ok(provider) => provider,
                    Err(_) => return (name, Err("authentication is required".into())),
                };
            let authentication = if native_codex_catalog(&name, &provider) {
                let token = provider.api_key.clone();
                let account_id = match tokio::task::spawn_blocking(move || {
                    crate::codex_auth::account_id_for_access_token(&token)
                })
                .await
                {
                    Ok(account_id) => account_id,
                    Err(_) => return (name, Err("could not resolve catalog account".into())),
                };
                CatalogAuthentication::Codex { account_id }
            } else if public_model_catalog(&provider) {
                CatalogAuthentication::Public
            } else if provider.protocol == crate::agent::ProviderProtocol::AnthropicMessages {
                CatalogAuthentication::Anthropic
            } else {
                CatalogAuthentication::Bearer
            };
            let result = fetch_provider_models(&client, &provider, authentication).await;
            (name, result)
        }
    });

    let mut refresh = CatalogRefresh {
        documented,
        ..CatalogRefresh::default()
    };
    let results: Vec<_> = requests
        .buffer_unordered(MAX_CONCURRENT_PROVIDERS)
        .collect()
        .await;
    for (provider, result) in results {
        match result {
            Ok(models) => {
                refresh.catalogs.insert(provider, models);
            }
            Err(error) => refresh.failures.push((provider, error)),
        }
    }
    refresh.failures.sort_by(|left, right| left.0.cmp(&right.0));
    refresh
}

pub(crate) fn documented_chat_models(name: &str, provider: &ProviderConfig) -> Vec<String> {
    match crate::provider_contract::route_family(name, provider) {
        Some(
            crate::provider_contract::FirstPartyProvider::OpenAiDirect
            | crate::provider_contract::FirstPartyProvider::OpenAiCodex,
        ) => vec![
            "gpt-5.6-sol".into(),
            "gpt-5.6-terra".into(),
            "gpt-5.6-luna".into(),
        ],
        Some(crate::provider_contract::FirstPartyProvider::Mimo) => {
            vec!["mimo-v2.5".into(), "mimo-v2.5-pro".into()]
        }
        Some(crate::provider_contract::FirstPartyProvider::DeepSeek) => {
            vec!["deepseek-v4-flash".into(), "deepseek-v4-pro".into()]
        }
        None => Vec::new(),
    }
}

pub(crate) fn native_codex_catalog(name: &str, provider: &ProviderConfig) -> bool {
    crate::provider_contract::route_family(name, provider)
        == Some(crate::provider_contract::FirstPartyProvider::OpenAiCodex)
}

fn public_model_catalog(provider: &ProviderConfig) -> bool {
    provider_host(provider).as_deref() == Some("api.ofox.ai")
}

fn provider_host(provider: &ProviderConfig) -> Option<String> {
    reqwest::Url::parse(provider.base_url.trim())
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
}

async fn fetch_provider_models(
    client: &reqwest::Client,
    provider: &ProviderConfig,
    authentication: CatalogAuthentication,
) -> Result<Vec<String>, String> {
    if provider.base_url.trim().is_empty() {
        return Err("base URL is empty".into());
    }
    if authentication != CatalogAuthentication::Public
        && provider.api_key.starts_with("${")
        && provider.api_key.ends_with('}')
    {
        return Err("API key environment variable is not set".into());
    }
    let url = format!("{}/models", provider.base_url.trim_end_matches('/'));
    let mut request = client.get(url);
    match &authentication {
        CatalogAuthentication::Codex { account_id } => {
            request = request
                .query(&[("client_version", CODEX_CATALOG_CLIENT_VERSION)])
                .header(
                    reqwest::header::USER_AGENT,
                    concat!("ca/", env!("CARGO_PKG_VERSION")),
                )
                .header("originator", "ca");
            if !provider.api_key.is_empty() {
                request = request.bearer_auth(&provider.api_key);
            }
            if let Some(account_id) = account_id {
                request = request.header("ChatGPT-Account-ID", account_id);
            }
        }
        CatalogAuthentication::Bearer if !provider.api_key.is_empty() => {
            request = request.bearer_auth(&provider.api_key);
        }
        CatalogAuthentication::Anthropic => {
            request = request.header("anthropic-version", "2023-06-01");
            if !provider.api_key.is_empty() {
                request = request.header("x-api-key", &provider.api_key);
            }
        }
        CatalogAuthentication::Bearer | CatalogAuthentication::Public => {}
    }
    let response = request.send().await.map_err(|error| {
        if error.is_timeout() {
            "request timed out".to_string()
        } else if error.is_connect() {
            "could not connect".to_string()
        } else {
            "request failed".to_string()
        }
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(match status.as_u16() {
            401 | 403 => format!("access denied (HTTP {status})"),
            404 => format!("models endpoint not found (HTTP {status})"),
            _ => format!("models request failed (HTTP {status})"),
        });
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_CATALOG_BYTES as u64)
    {
        return Err("model catalog exceeded 1 MiB".into());
    }

    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "model response failed".to_string())?;
        if body.len().saturating_add(chunk.len()) > MAX_CATALOG_BYTES {
            return Err("model catalog exceeded 1 MiB".into());
        }
        body.extend_from_slice(&chunk);
    }
    let ids: Vec<String> = if matches!(authentication, CatalogAuthentication::Codex { .. }) {
        let decoded: CodexModelListResponse = serde_json::from_slice(&body)
            .map_err(|_| "provider returned an invalid model catalog".to_string())?;
        decoded
            .models
            .into_iter()
            .filter(|model| model.visibility.as_deref() == Some("list"))
            .map(|model| model.slug)
            .collect()
    } else {
        let decoded: ModelListResponse = serde_json::from_slice(&body)
            .map_err(|_| "provider returned an invalid model catalog".to_string())?;
        decoded
            .data
            .into_iter()
            .filter(|model| {
                authentication != CatalogAuthentication::Public || model.is_agent_compatible()
            })
            .map(|model| model.id)
            .collect()
    };
    let mut models: Vec<String> = ids
        .into_iter()
        .filter(|id| valid_model_id(id))
        .take(MAX_MODEL_COUNT)
        .collect();
    models.sort();
    models.dedup();
    Ok(models)
}

fn valid_model_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_MODEL_ID_BYTES
        && id.trim() == id
        && !id.chars().any(char::is_control)
}

#[cfg(test)]
pub(crate) fn codex_fixture_models() -> Vec<String> {
    let fixture: CodexModelListResponse =
        serde_json::from_slice(include_bytes!("../tests/fixtures/codex-model-catalog.json"))
            .unwrap();
    let mut models: Vec<_> = fixture
        .models
        .into_iter()
        .filter(|model| model.visibility.as_deref() == Some("list"))
        .map(|model| model.slug)
        .collect();
    models.sort();
    models
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderTimeoutConfig;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn native_codex_catalog_includes_seven_visible_models() {
        let (url, request) = one_shot_server(
            "200 OK",
            include_bytes!("../tests/fixtures/codex-model-catalog.json").to_vec(),
        )
        .await;
        let result = fetch_provider_models(
            &reqwest::Client::new(),
            &provider(url, "test-token"),
            CatalogAuthentication::Codex {
                account_id: Some("test-account".into()),
            },
        )
        .await;
        let request = request.await.unwrap().to_ascii_lowercase();
        assert!(request.starts_with("get /v1/models?client_version=0.153.0 http/1.1"));
        assert!(request.contains("authorization: bearer test-token\r\n"));
        assert!(request.contains("chatgpt-account-id: test-account\r\n"));
        assert!(request.contains(concat!(
            "user-agent: ca/",
            env!("CARGO_PKG_VERSION"),
            "\r\n"
        )));
        assert!(request.contains("originator: ca\r\n"));
        assert_eq!(
            result.unwrap(),
            [
                "gpt-5.3-codex-spark",
                "gpt-5.4",
                "gpt-5.4-mini",
                "gpt-5.5",
                "gpt-5.6-luna",
                "gpt-5.6-sol",
                "gpt-5.6-terra"
            ]
        );
    }

    #[tokio::test]
    async fn native_codex_catalog_validates_visibility_ids_count_and_empty_success() {
        let mut entries = vec![
            serde_json::json!({"slug": "bad\nmodel", "visibility": "list"}),
            serde_json::json!({"slug": " padded ", "visibility": "list"}),
            serde_json::json!({"slug": "", "visibility": "list"}),
            serde_json::json!({"slug": "x".repeat(MAX_MODEL_ID_BYTES + 1), "visibility": "list"}),
            serde_json::json!({"slug": "hidden", "visibility": "hide"}),
            serde_json::json!({"slug": "unknown-visibility", "visibility": "future"}),
            serde_json::json!({"slug": "unspecified"}),
        ];
        entries.extend((0..MAX_MODEL_COUNT + 1).map(
            |index| serde_json::json!({"slug": format!("model-{index:04}"), "visibility": "list"}),
        ));
        for (body, expected) in [
            (serde_json::json!({"models": entries}), MAX_MODEL_COUNT),
            (serde_json::json!({"models": []}), 0),
            (
                serde_json::json!({"models": [
                    {"slug":"duplicate", "visibility":"list"},
                    {"slug":"duplicate", "visibility":"list"}
                ]}),
                1,
            ),
        ] {
            let (url, request) =
                one_shot_server("200 OK", serde_json::to_vec(&body).unwrap()).await;
            let result = fetch_provider_models(
                &reqwest::Client::new(),
                &provider(url, "test-token"),
                CatalogAuthentication::Codex { account_id: None },
            )
            .await
            .unwrap();
            assert_eq!(result.len(), expected);
            assert!(!request
                .await
                .unwrap()
                .to_ascii_lowercase()
                .contains("chatgpt-account-id:"));
        }
    }

    #[tokio::test]
    async fn native_codex_catalog_rejects_malformed_oversized_and_http_failures_safely() {
        for (status, body, expected) in [
            (
                "200 OK",
                b"invalid private-body".to_vec(),
                "invalid model catalog",
            ),
            (
                "200 OK",
                br#"{"data":[{"id":"wrong-format"}]}"#.to_vec(),
                "invalid model catalog",
            ),
            (
                "200 OK",
                br#"{"models":[{"visibility":"list"}]}"#.to_vec(),
                "invalid model catalog",
            ),
            (
                "200 OK",
                vec![b'x'; MAX_CATALOG_BYTES + 1],
                "exceeded 1 MiB",
            ),
            ("400 Bad Request", b"private-body".to_vec(), "HTTP 400"),
            (
                "401 Unauthorized",
                b"private-body".to_vec(),
                "access denied",
            ),
            ("403 Forbidden", b"private-body".to_vec(), "access denied"),
            (
                "404 Not Found",
                b"private-body".to_vec(),
                "endpoint not found",
            ),
            (
                "429 Too Many Requests",
                b"private-body".to_vec(),
                "HTTP 429",
            ),
            (
                "503 Service Unavailable",
                b"private-body".to_vec(),
                "HTTP 503",
            ),
        ] {
            let (url, request) = one_shot_server(status, body).await;
            let error = fetch_provider_models(
                &reqwest::Client::new(),
                &provider(url, "private-token"),
                CatalogAuthentication::Codex {
                    account_id: Some("private-account".into()),
                },
            )
            .await
            .unwrap_err();
            request.await.unwrap();
            assert!(error.contains(expected), "{error}");
            assert!(!error.contains("private"));
        }
    }

    #[tokio::test]
    async fn native_codex_catalog_bounds_streamed_bodies_without_content_length() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 8192];
            let length = stream.read(&mut request).await.unwrap();
            assert!(length > 0);
            let _ = stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await;
            let body = vec![b'x'; MAX_CATALOG_BYTES + 1];
            let _ = stream
                .write_all(format!("{:x}\r\n", body.len()).as_bytes())
                .await;
            let _ = stream.write_all(&body).await;
            let _ = stream.write_all(b"\r\n0\r\n\r\n").await;
        });
        let error = fetch_provider_models(
            &reqwest::Client::new(),
            &provider(url, "test-token"),
            CatalogAuthentication::Codex { account_id: None },
        )
        .await
        .unwrap_err();
        server.await.unwrap();
        assert_eq!(error, "model catalog exceeded 1 MiB");
    }

    #[test]
    fn native_codex_catalog_is_exact_route_only() {
        let exact =
            crate::provider_connect::FirstClassProvider::OpenAiCodex.candidate(String::new(), None);
        assert!(native_codex_catalog("openai-codex", &exact));
        assert!(!native_codex_catalog("openai", &exact));
        for url in [
            "https://example.invalid/backend-api/codex",
            "https://chatgpt.com/backend-api/codex/custom",
            "https://chatgpt.com/backend-api/codex?custom=1",
            "http://chatgpt.com/backend-api/codex",
        ] {
            let mut custom = exact.clone();
            custom.base_url = url.into();
            assert!(!native_codex_catalog("openai-codex", &custom), "{url}");
        }
    }

    #[tokio::test]
    async fn native_codex_catalog_timeout_is_sanitized() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();
        let error = fetch_provider_models(
            &client,
            &provider(url, "private-token"),
            CatalogAuthentication::Codex {
                account_id: Some("private-account".into()),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error, "request timed out");
    }

    #[tokio::test]
    async fn custom_codex_catalog_keeps_generic_request_and_decoder() {
        let (url, request) =
            one_shot_server("200 OK", br#"{"data":[{"id":"custom-model"}]}"#.to_vec()).await;
        let custom = crate::provider_connect::FirstClassProvider::OpenAiCodex
            .candidate("test-token".into(), Some(url));
        let refresh =
            discover_provider_models(HashMap::from([("openai-codex".into(), custom)])).await;
        assert!(refresh.failures.is_empty());
        assert_eq!(refresh.catalogs["openai-codex"], ["custom-model"]);
        assert!(refresh.documented.is_empty());
        let request = request.await.unwrap().to_ascii_lowercase();
        assert!(request.starts_with("get /v1/models http/1.1"));
        assert!(!request.contains("client_version"));
        assert!(!request.contains("chatgpt-account-id"));
        assert!(!request.contains("originator"));
    }

    async fn one_shot_server(
        status: &str,
        body: Vec<u8>,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_string();
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 8192];
            let length = stream.read(&mut request).await.unwrap();
            request.truncate(length);
            let headers = String::from_utf8_lossy(&request).into_owned();
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.write_all(&body).await;
            headers
        });
        (format!("http://{address}/v1"), handle)
    }

    fn provider(base_url: String, api_key: &str) -> ProviderConfig {
        ProviderConfig {
            base_url,
            api_key: api_key.into(),
            protocol: crate::agent::ProviderProtocol::default(),
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["configured".into()],
            timeout: ProviderTimeoutConfig::default(),
        }
    }

    #[tokio::test]
    async fn lists_authenticated_provider_models_sorted_and_deduplicated() {
        let body = br#"{"object":"list","data":[{"id":"mimo-z"},{"id":"mimo-a"},{"id":"mimo-a"},{"id":"bad\nmodel"}]}"#.to_vec();
        let (base_url, request) = one_shot_server("200 OK", body).await;
        let mut providers = HashMap::new();
        providers.insert("mimo".into(), provider(base_url, "test-secret"));

        let refresh = discover_provider_models(providers).await;

        assert_eq!(refresh.catalogs["mimo"], ["mimo-a", "mimo-z"]);
        assert!(refresh.failures.is_empty());
        let request = request.await.unwrap();
        assert!(request.starts_with("GET /v1/models HTTP/1.1"));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer test-secret"));
    }

    #[tokio::test]
    async fn anthropic_catalog_uses_versioned_key_headers_without_bearer_auth() {
        let (base_url, request) =
            one_shot_server("200 OK", br#"{"data":[{"id":"claude-test"}]}"#.to_vec()).await;
        let mut anthropic = provider(base_url, "test-secret");
        anthropic.protocol = crate::agent::ProviderProtocol::AnthropicMessages;
        let mut providers = HashMap::new();
        providers.insert("anthropic".into(), anthropic);

        let refresh = discover_provider_models(providers).await;

        assert_eq!(refresh.catalogs["anthropic"], ["claude-test"]);
        let request = request.await.unwrap();
        assert!(request.contains("x-api-key: test-secret"));
        assert!(request.contains("anthropic-version: 2023-06-01"));
        assert!(!request.to_ascii_lowercase().contains("authorization:"));
    }

    #[tokio::test]
    async fn partial_failure_keeps_success_and_reports_only_bounded_status() {
        let (good_url, good_request) =
            one_shot_server("200 OK", br#"{"data":[{"id":"available"}]}"#.to_vec()).await;
        let (bad_url, bad_request) = one_shot_server(
            "401 Unauthorized",
            br#"{"error":"do not surface this body"}"#.to_vec(),
        )
        .await;
        let mut providers = HashMap::new();
        providers.insert("good".into(), provider(good_url, "good-key"));
        providers.insert("bad".into(), provider(bad_url, "secret-bad-key"));

        let refresh = discover_provider_models(providers).await;

        assert_eq!(refresh.catalogs["good"], ["available"]);
        assert_eq!(refresh.failures.len(), 1);
        assert_eq!(refresh.failures[0].0, "bad");
        assert!(refresh.failures[0].1.contains("HTTP 401"));
        assert!(!refresh.failures[0].1.contains("surface"));
        assert!(!refresh.failures[0].1.contains("secret-bad-key"));
        good_request.await.unwrap();
        bad_request.await.unwrap();
    }

    #[tokio::test]
    async fn unresolved_keys_malformed_json_and_oversize_catalogs_fail_closed() {
        let mut unresolved = HashMap::new();
        unresolved.insert(
            "missing".into(),
            provider("https://example.invalid/v1".into(), "${MISSING_KEY}"),
        );
        let refresh = discover_provider_models(unresolved).await;
        assert!(refresh.catalogs.is_empty());
        assert!(refresh.failures[0]
            .1
            .contains("environment variable is not set"));

        let (malformed_url, malformed_request) =
            one_shot_server("200 OK", b"not-json".to_vec()).await;
        let mut malformed = HashMap::new();
        malformed.insert("bad".into(), provider(malformed_url, "key"));
        let refresh = discover_provider_models(malformed).await;
        assert!(refresh.failures[0].1.contains("invalid model catalog"));
        malformed_request.await.unwrap();

        let (large_url, large_request) =
            one_shot_server("200 OK", vec![b'x'; MAX_CATALOG_BYTES + 1]).await;
        let mut large = HashMap::new();
        large.insert("large".into(), provider(large_url, "key"));
        let refresh = discover_provider_models(large).await;
        assert!(refresh.failures[0].1.contains("exceeded 1 MiB"));
        large_request.await.unwrap();
    }

    #[tokio::test]
    async fn mimo_keeps_documented_chat_models_when_live_discovery_is_unavailable() {
        let mut providers = HashMap::new();
        providers.insert(
            "mimo".into(),
            provider(
                "https://token-plan-sgp.xiaomimimo.com/v1".into(),
                "${MIMO_API_KEY}",
            ),
        );

        let refresh = discover_provider_models(providers).await;

        assert_eq!(refresh.documented["mimo"], ["mimo-v2.5", "mimo-v2.5-pro"]);
        assert!(refresh.catalogs.is_empty());
        assert_eq!(refresh.failures[0].0, "mimo");
    }

    #[test]
    fn unrelated_providers_do_not_receive_inferred_models() {
        assert!(documented_chat_models(
            "custom",
            &provider("https://example.com/v1".into(), "key")
        )
        .is_empty());
    }

    #[test]
    fn deepseek_has_current_documented_chat_models() {
        assert_eq!(
            documented_chat_models(
                "deepseek",
                &provider("https://api.deepseek.com/v1".into(), "${DEEPSEEK_API_KEY}")
            ),
            ["deepseek-v4-flash", "deepseek-v4-pro"]
        );
    }

    #[test]
    fn direct_openai_catalog_is_exact_and_named_proxies_receive_no_built_ins() {
        let mut direct = provider("https://api.openai.com/v1".into(), "${OPENAI_API_KEY}");
        direct.protocol = crate::agent::ProviderProtocol::OpenAiResponses;
        assert_eq!(
            documented_chat_models("openai", &direct),
            ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"]
        );
        direct.base_url = "https://proxy.example/v1".into();
        assert!(documented_chat_models("openai", &direct).is_empty());
    }

    #[test]
    fn codex_catalog_falls_back_to_supported_openai_models_only_on_the_exact_route() {
        let mut codex = provider(
            "https://chatgpt.com/backend-api/codex".into(),
            "oauth-token",
        );
        codex.protocol = crate::agent::ProviderProtocol::OpenAiResponses;
        assert_eq!(
            documented_chat_models("openai-codex", &codex),
            ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"]
        );

        codex.base_url = "https://proxy.example/backend-api/codex".into();
        assert!(documented_chat_models("openai-codex", &codex).is_empty());
    }

    #[tokio::test]
    async fn public_catalog_can_refresh_without_sending_an_api_key() {
        assert!(public_model_catalog(&provider(
            "https://api.ofox.ai/v1".into(),
            "${OFOX_API_KEY}"
        )));

        let body = br#"{
            "data": [
                {
                    "id": "current-chat-model",
                    "architecture": {"output_modalities": ["text"]},
                    "supported_parameters": ["tools"]
                },
                {
                    "id": "video-only-model",
                    "architecture": {"output_modalities": ["video"]},
                    "supported_parameters": ["tools"]
                },
                {
                    "id": "text-without-tools",
                    "architecture": {"output_modalities": ["text"]},
                    "supported_parameters": ["temperature"]
                }
            ]
        }"#
        .to_vec();
        let (base_url, request) = one_shot_server("200 OK", body).await;
        let client = reqwest::Client::new();
        let models = fetch_provider_models(
            &client,
            &provider(base_url, "${OFOX_API_KEY}"),
            CatalogAuthentication::Public,
        )
        .await
        .unwrap();

        assert_eq!(models, ["current-chat-model"]);
        assert!(!request
            .await
            .unwrap()
            .to_ascii_lowercase()
            .contains("authorization:"));
    }

    #[tokio::test]
    async fn connection_failures_do_not_expose_catalog_urls() {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        let provider = provider(
            "http://127.0.0.1:0/private?token=do-not-display".into(),
            "secret-key",
        );

        let error = fetch_provider_models(&client, &provider, CatalogAuthentication::Bearer)
            .await
            .unwrap_err();

        assert_eq!(error, "could not connect");
        assert!(!error.contains("do-not-display"));
        assert!(!error.contains("secret-key"));
    }
}
