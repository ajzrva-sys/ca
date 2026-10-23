use crate::agent::ProviderProtocol;
use crate::config::{ConfigStore, ProviderConfig, ProviderTimeoutConfig};
use crate::provider_probe::{ProviderCheckStatus, ProviderProbeRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstClassProvider {
    Mimo,
    OpenAiCodex,
    DeepSeek,
    OpenAi,
}

impl FirstClassProvider {
    pub fn for_id(id: &str) -> Option<Self> {
        match id {
            "mimo" => Some(Self::Mimo),
            "openai-codex" => Some(Self::OpenAiCodex),
            "deepseek" => Some(Self::DeepSeek),
            "openai" => Some(Self::OpenAi),
            _ => None,
        }
    }

    pub const fn id(self) -> &'static str {
        match self {
            Self::Mimo => "mimo",
            Self::OpenAiCodex => "openai-codex",
            Self::DeepSeek => "deepseek",
            Self::OpenAi => "openai",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Mimo => "Xiaomi MiMo",
            Self::OpenAiCodex => "OpenAI Codex (ChatGPT)",
            Self::DeepSeek => "DeepSeek",
            Self::OpenAi => "OpenAI API",
        }
    }

    pub const fn starter_model(self) -> &'static str {
        match self {
            Self::Mimo => "mimo-v2.5-pro",
            Self::OpenAiCodex | Self::OpenAi => "gpt-5.6-sol",
            Self::DeepSeek => "deepseek-v4-pro",
        }
    }

    pub const fn default_base_url(self) -> &'static str {
        match self {
            Self::Mimo => "https://api.xiaomimimo.com/v1",
            Self::OpenAiCodex => "https://chatgpt.com/backend-api/codex",
            Self::DeepSeek => "https://api.deepseek.com/v1",
            Self::OpenAi => "https://api.openai.com/v1",
        }
    }

    pub const fn protocol(self) -> ProviderProtocol {
        match self {
            Self::Mimo | Self::OpenAiCodex | Self::OpenAi => ProviderProtocol::OpenAiResponses,
            Self::DeepSeek => ProviderProtocol::ChatCompletions,
        }
    }

    pub fn candidate(self, api_key: String, base_url: Option<String>) -> ProviderConfig {
        ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: base_url.unwrap_or_else(|| self.default_base_url().to_string()),
            api_key,
            protocol: self.protocol(),
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec![self.starter_model().to_string()],
            timeout: ProviderTimeoutConfig::default(),
        }
    }
}

#[derive(Debug)]
pub struct ProviderConnectionOutcome {
    pub provider: FirstClassProvider,
    pub status: ProviderCheckStatus,
    pub config: ProviderConfig,
    pub persisted: bool,
    pub selected: bool,
    pub billing: Option<crate::pricing::ProviderBillingConfig>,
    pub billing_repair: bool,
}

impl ProviderConnectionOutcome {
    pub fn retained(&self) -> bool {
        !authentication_rejected(&self.status)
    }
}

pub async fn resolve_runtime_provider(
    provider: &str,
    mut config: ProviderConfig,
) -> Result<ProviderConfig, String> {
    if provider == FirstClassProvider::OpenAiCodex.id() && config.api_key.trim().is_empty() {
        config.api_key = crate::codex_auth::resolve_credential()
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(config)
}

pub async fn connect_direct(
    store: ConfigStore,
    provider: FirstClassProvider,
    config: ProviderConfig,
    secret: crate::provider_secrets::SecretValue,
    request_store: crate::request::RequestStore,
) -> Result<ProviderConnectionOutcome, String> {
    connect_direct_with_billing(store, provider, config, secret, request_store, None).await
}

pub async fn connect_direct_with_billing(
    store: ConfigStore,
    provider: FirstClassProvider,
    config: ProviderConfig,
    secret: crate::provider_secrets::SecretValue,
    request_store: crate::request::RequestStore,
    billing: Option<crate::pricing::ProviderBillingConfig>,
) -> Result<ProviderConnectionOutcome, String> {
    validate_mimo_connection(provider, &config, billing.as_ref())?;
    let outcome_billing = billing.clone();
    let model = provider.starter_model().to_string();
    if !store.writes_enabled() {
        let status = probe(provider, &model, config.clone(), request_store).await;
        return Ok(ProviderConnectionOutcome {
            provider,
            selected: status.working_model().is_some(),
            persisted: false,
            status,
            config,
            billing: outcome_billing,
            billing_repair: false,
        });
    }
    let stage_store = store.clone();
    let stage_config = config.clone();
    let stage_model = model.clone();
    let stage_provider = provider.id().to_string();
    let rollback = tokio::task::spawn_blocking(move || {
        stage_store.stage_authenticated_provider_with_billing(
            &stage_provider,
            &stage_config,
            &stage_model,
            Some(&secret),
            billing.as_ref(),
        )
    })
    .await
    .map_err(|_| "credential save task failed".to_string())?
    .map_err(|error| error.to_string())?;
    let status = probe(provider, &model, config.clone(), request_store).await;
    if authentication_rejected(&status) {
        let rollback_store = store.clone();
        tokio::task::spawn_blocking(move || {
            rollback_store.rollback_authenticated_provider(rollback)
        })
        .await
        .map_err(|_| "credential rollback task failed".to_string())?
        .map_err(|error| format!("credential was rejected and rollback failed: {error}"))?;
        let mut config = config;
        config.api_key.clear();
        return Ok(ProviderConnectionOutcome {
            provider,
            status,
            config,
            persisted: false,
            selected: false,
            billing: None,
            billing_repair: false,
        });
    }
    let selected = status.working_model().is_some();
    if selected {
        let selection_store = store.clone();
        let selection_provider = provider.id().to_string();
        let selection_model = model.clone();
        if let Err(error) = tokio::task::spawn_blocking(move || {
            selection_store.select_model(&selection_provider, &selection_model)
        })
        .await
        .map_err(|_| "default selection task failed".to_string())?
        {
            let rollback_store = store.clone();
            tokio::task::spawn_blocking(move || {
                rollback_store.rollback_authenticated_provider(rollback)
            })
            .await
            .map_err(|_| "credential rollback task failed".to_string())?
            .map_err(|rollback_error| {
                format!(
                    "could not select provider: {error}; rollback also failed: {rollback_error}"
                )
            })?;
            return Err(format!("could not select provider: {error}"));
        }
    }
    let mut config = config;
    config.api_key.clear();
    Ok(ProviderConnectionOutcome {
        provider,
        status,
        config,
        persisted: true,
        selected,
        billing: outcome_billing,
        billing_repair: false,
    })
}

pub async fn repair_existing_mimo_billing(
    store: ConfigStore,
    billing: crate::pricing::ProviderBillingConfig,
    request_store: crate::request::RequestStore,
) -> Result<ProviderConnectionOutcome, String> {
    if !store.writes_enabled() {
        return Err("Saved Token Plan repair is unavailable in bare mode".into());
    }
    let load_store = store.clone();
    let loaded = tokio::task::spawn_blocking(move || load_store.load())
        .await
        .map_err(|_| "saved MiMo credential load task failed".to_string())?
        .map_err(|error| format!("saved MiMo credential could not be resolved: {error}"))?;
    let provider = FirstClassProvider::Mimo;
    let mut config = loaded
        .providers
        .get(provider.id())
        .cloned()
        .ok_or_else(|| "saved MiMo provider is no longer configured".to_string())?;
    validate_mimo_connection(provider, &config, Some(&billing))?;
    let model = provider.starter_model().to_string();
    if !config.models.iter().any(|candidate| candidate == &model) {
        return Err("saved MiMo provider does not include mimo-v2.5-pro".into());
    }

    let status = probe(provider, &model, config.clone(), request_store).await;
    config.api_key.clear();
    if status.working_model().is_none() {
        return Ok(ProviderConnectionOutcome {
            provider,
            status,
            config,
            persisted: false,
            selected: false,
            billing: None,
            billing_repair: true,
        });
    }
    let update_store = store.clone();
    let update_billing = billing.clone();
    tokio::task::spawn_blocking(move || {
        update_store.update_provider_billing(provider.id(), &update_billing)
    })
    .await
    .map_err(|_| "MiMo billing save task failed".to_string())?
    .map_err(|error| format!("MiMo probe passed but billing metadata was not saved: {error}"))?;
    Ok(ProviderConnectionOutcome {
        provider,
        status,
        config,
        persisted: true,
        selected: false,
        billing: Some(billing),
        billing_repair: true,
    })
}

fn validate_mimo_connection(
    provider: FirstClassProvider,
    config: &ProviderConfig,
    billing: Option<&crate::pricing::ProviderBillingConfig>,
) -> Result<(), String> {
    if provider != FirstClassProvider::Mimo {
        if billing.is_some() {
            return Err("billing metadata is unsupported for this provider".into());
        }
        return Ok(());
    }
    match crate::mimo_auth::classify_credential(config.api_key.as_bytes(), &config.base_url)
        .map_err(|error| error.to_string())?
    {
        crate::mimo_auth::MimoCredentialKind::Payg if billing.is_none() => Ok(()),
        crate::mimo_auth::MimoCredentialKind::TokenPlan(region) => {
            let billing = billing.ok_or_else(|| {
                "MiMo Token Plan allocation metadata is required before connection".to_string()
            })?;
            billing
                .validate_for_provider(provider.id(), &config.base_url)
                .map_err(|_| "MiMo Token Plan billing metadata was inconsistent".to_string())?;
            if crate::pricing::MimoRegion::from_token_plan_url(&config.base_url) != Some(region) {
                return Err("MiMo Token Plan region was inconsistent".into());
            }
            Ok(())
        }
        crate::mimo_auth::MimoCredentialKind::Payg => {
            Err("PAYG MiMo credentials cannot use Token Plan billing metadata".into())
        }
    }
}

pub async fn connect_codex(
    store: ConfigStore,
    tokens: crate::codex_auth::CodexTokens,
    request_store: crate::request::RequestStore,
) -> Result<ProviderConnectionOutcome, String> {
    let provider = FirstClassProvider::OpenAiCodex;
    let model = provider.starter_model().to_string();
    let config = provider.candidate(tokens.access_token.clone(), None);
    let status = probe(provider, &model, config.clone(), request_store).await;
    if authentication_rejected(&status) {
        return Ok(ProviderConnectionOutcome {
            provider,
            status,
            config,
            persisted: false,
            selected: false,
            billing: None,
            billing_repair: false,
        });
    }
    if !store.writes_enabled() {
        return Ok(ProviderConnectionOutcome {
            provider,
            selected: status.working_model().is_some(),
            persisted: false,
            status,
            config,
            billing: None,
            billing_repair: false,
        });
    }
    let previous_tokens = crate::codex_auth::load_tokens().map_err(|error| error.to_string())?;
    crate::codex_auth::store_tokens(&tokens).map_err(|error| error.to_string())?;
    let persisted_config = provider.candidate(String::new(), None);
    let stage_store = store.clone();
    let stage_config = persisted_config.clone();
    let stage = tokio::task::spawn_blocking(move || {
        stage_store.stage_authenticated_provider(
            provider.id(),
            &stage_config,
            provider.starter_model(),
            None,
        )
    })
    .await
    .map_err(|_| "provider save task failed".to_string())?
    .map_err(|error| error.to_string());
    let rollback = match stage {
        Ok(rollback) => rollback,
        Err(error) => {
            restore_codex_tokens(previous_tokens.as_ref())?;
            return Err(error);
        }
    };
    let selected = status.working_model().is_some();
    if selected {
        let selection_store = store.clone();
        let selection = tokio::task::spawn_blocking(move || {
            selection_store.select_model(provider.id(), provider.starter_model())
        })
        .await
        .map_err(|_| "default selection task failed".to_string())?;
        if let Err(error) = selection {
            store
                .rollback_authenticated_provider(rollback)
                .map_err(|rollback| format!("{error}; config rollback failed: {rollback}"))?;
            restore_codex_tokens(previous_tokens.as_ref())?;
            return Err(format!("could not select provider: {error}"));
        }
    }
    Ok(ProviderConnectionOutcome {
        provider,
        status,
        config: persisted_config,
        persisted: true,
        selected,
        billing: None,
        billing_repair: false,
    })
}

fn restore_codex_tokens(previous: Option<&crate::codex_auth::CodexTokens>) -> Result<(), String> {
    match previous {
        Some(tokens) => crate::codex_auth::store_tokens(tokens),
        None => crate::codex_auth::delete_tokens(),
    }
    .map_err(|error| format!("Codex credential rollback failed: {error}"))
}

async fn probe(
    provider: FirstClassProvider,
    model: &str,
    config: ProviderConfig,
    request_store: crate::request::RequestStore,
) -> ProviderCheckStatus {
    crate::provider_probe::probe_provider(ProviderProbeRequest {
        provider: provider.id().into(),
        model: model.into(),
        config,
        usage: None,
        request_store,
    })
    .await
    .status
}

fn authentication_rejected(status: &ProviderCheckStatus) -> bool {
    matches!(
        status,
        ProviderCheckStatus::CredentialRejected | ProviderCheckStatus::AccessDenied
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_secrets::ProviderSecretStore;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[derive(Default)]
    struct FakeProviderSecrets {
        values: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl ProviderSecretStore for FakeProviderSecrets {
        fn load(&self, account: &str) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self.values.lock().unwrap().get(account).cloned())
        }

        fn store(&self, account: &str, secret: &[u8]) -> anyhow::Result<()> {
            self.values
                .lock()
                .unwrap()
                .insert(account.to_string(), secret.to_vec());
            Ok(())
        }

        fn delete(&self, account: &str) -> anyhow::Result<()> {
            self.values.lock().unwrap().remove(account);
            Ok(())
        }
    }

    async fn one_shot_server(status: &str, body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_string();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 16 * 1024];
            let _ = stream.read(&mut request).await.unwrap();
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        format!("http://{address}/v1")
    }

    fn complete_stream() -> &'static str {
        concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        )
    }

    fn persisted_store(raw: &str) -> (tempfile::TempDir, ConfigStore, Arc<FakeProviderSecrets>) {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        std::fs::write(&config_path, raw).unwrap();
        let secrets = Arc::new(FakeProviderSecrets::default());
        let store = ConfigStore::with_secret_store(&config_path, secrets.clone());
        (temp, store, secrets)
    }

    #[test]
    fn first_class_specs_are_exact_and_distinct() {
        assert_eq!(
            FirstClassProvider::for_id("mimo").unwrap().display_name(),
            "Xiaomi MiMo"
        );
        assert_eq!(
            FirstClassProvider::OpenAiCodex.default_base_url(),
            "https://chatgpt.com/backend-api/codex"
        );
        assert_eq!(
            FirstClassProvider::DeepSeek.starter_model(),
            "deepseek-v4-pro"
        );
        assert_eq!(
            FirstClassProvider::OpenAi.protocol(),
            ProviderProtocol::OpenAiResponses
        );
        assert!(FirstClassProvider::for_id("openai-api").is_none());
    }

    #[tokio::test]
    async fn mimo_credential_endpoint_mismatches_fail_before_traffic_or_persistence() {
        let original = r#"[providers.backup]
base_url = "https://backup.example/v1"
api_key = "${BACKUP_API_KEY}"
models = ["backup-model"]

[defaults]
provider = "backup"
model = "backup-model"
"#;
        for (key, endpoint) in [
            (
                "tp-secret-never-render",
                FirstClassProvider::Mimo.default_base_url(),
            ),
            (
                "sk-secret-never-render",
                crate::pricing::MimoRegion::Singapore.token_plan_url(),
            ),
        ] {
            let (_temp, store, secrets) = persisted_store(original);
            let result = connect_direct(
                store.clone(),
                FirstClassProvider::Mimo,
                FirstClassProvider::Mimo.candidate(key.into(), Some(endpoint.into())),
                crate::provider_secrets::SecretValue::from_text(key),
                crate::request::RequestStore::default(),
            )
            .await;

            let error = result.unwrap_err();
            assert!(error.contains("inconsistent"));
            assert!(!error.contains("secret-never-render"));
            assert_eq!(std::fs::read_to_string(store.path()).unwrap(), original);
            assert!(secrets.values.lock().unwrap().is_empty());
        }

        let (_temp, store, secrets) = persisted_store(original);
        let result = connect_direct(
            store.clone(),
            FirstClassProvider::Mimo,
            FirstClassProvider::Mimo.candidate(
                "tp-secret-never-render".into(),
                Some(
                    crate::pricing::MimoRegion::Singapore
                        .token_plan_url()
                        .into(),
                ),
            ),
            crate::provider_secrets::SecretValue::from_text("tp-secret-never-render"),
            crate::request::RequestStore::default(),
        )
        .await;
        let error = result.unwrap_err();
        assert!(error.contains("allocation metadata is required"));
        assert!(!error.contains("secret-never-render"));
        assert_eq!(std::fs::read_to_string(store.path()).unwrap(), original);
        assert!(secrets.values.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn rejected_replacement_restores_previous_config_and_secret() {
        let original = r#"[providers.deepseek]
base_url = "https://old.example/v1"
api_key = "credential://ca/provider/deepseek"
models = ["old-model"]

[defaults]
provider = "deepseek"
model = "old-model"
"#;
        let (_temp, store, secrets) = persisted_store(original);
        secrets.store("deepseek", b"previous-secret").unwrap();
        let base_url = one_shot_server("401 Unauthorized", "").await;
        let candidate =
            FirstClassProvider::DeepSeek.candidate("rejected-secret".into(), Some(base_url));

        let outcome = connect_direct(
            store.clone(),
            FirstClassProvider::DeepSeek,
            candidate,
            crate::provider_secrets::SecretValue::from_text("rejected-secret"),
            crate::request::RequestStore::default(),
        )
        .await
        .unwrap();

        assert_eq!(outcome.status, ProviderCheckStatus::CredentialRejected);
        assert!(!outcome.persisted);
        assert!(!outcome.selected);
        assert_eq!(std::fs::read_to_string(store.path()).unwrap(), original);
        assert_eq!(
            secrets.load("deepseek").unwrap().unwrap(),
            b"previous-secret"
        );
        assert!(!format!("{outcome:?}").contains("rejected-secret"));
    }

    #[tokio::test]
    async fn provider_failure_retains_new_credential_without_switching_default() {
        let original = r#"[providers.backup]
base_url = "https://backup.example/v1"
api_key = "${BACKUP_API_KEY}"
models = ["backup-model"]

[defaults]
provider = "backup"
model = "backup-model"
"#;
        let (_temp, store, secrets) = persisted_store(original);
        let base_url = one_shot_server("404 Not Found", "").await;
        let candidate = FirstClassProvider::DeepSeek.candidate("new-secret".into(), Some(base_url));

        let outcome = connect_direct(
            store.clone(),
            FirstClassProvider::DeepSeek,
            candidate,
            crate::provider_secrets::SecretValue::from_text("new-secret"),
            crate::request::RequestStore::default(),
        )
        .await
        .unwrap();

        assert_eq!(outcome.status, ProviderCheckStatus::EndpointNotFound);
        assert!(outcome.persisted);
        assert!(!outcome.selected);
        assert_eq!(secrets.load("deepseek").unwrap().unwrap(), b"new-secret");
        let raw = std::fs::read_to_string(store.path()).unwrap();
        assert!(raw.contains("credential://ca/provider/deepseek"));
        let config = store.load().unwrap();
        assert_eq!(config.defaults.provider, "backup");
        assert_eq!(config.providers["deepseek"].models, ["deepseek-v4-pro"]);
        assert!(!format!("{outcome:?}").contains("new-secret"));
    }

    #[tokio::test]
    async fn successful_connection_persists_and_selects_starter_model() {
        let original = r#"[providers.backup]
base_url = "https://backup.example/v1"
api_key = "${BACKUP_API_KEY}"
models = ["backup-model"]

[defaults]
provider = "backup"
model = "backup-model"
"#;
        let (_temp, store, secrets) = persisted_store(original);
        let base_url = one_shot_server("200 OK", complete_stream()).await;
        let candidate =
            FirstClassProvider::DeepSeek.candidate("working-secret".into(), Some(base_url));

        let outcome = connect_direct(
            store.clone(),
            FirstClassProvider::DeepSeek,
            candidate,
            crate::provider_secrets::SecretValue::from_text("working-secret"),
            crate::request::RequestStore::default(),
        )
        .await
        .unwrap();

        assert!(outcome.status.working_model().is_some());
        assert!(outcome.persisted);
        assert!(outcome.selected);
        assert_eq!(
            secrets.load("deepseek").unwrap().unwrap(),
            b"working-secret"
        );
        let config = store.load().unwrap();
        assert_eq!(config.defaults.provider, "deepseek");
        assert_eq!(config.defaults.model, "deepseek-v4-pro");
        assert!(!format!("{outcome:?}").contains("working-secret"));
    }
}
