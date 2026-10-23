//! Google connection settings and async ADC headers. Tokens never enter configuration.
use crate::agent::stream::ProviderError;
use google_cloud_auth::credentials::{Builder, CacheableResource, Credentials};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub(crate) fn is_vertex(provider: &crate::config::ProviderConfig) -> bool {
    provider.protocol == crate::agent::ProviderProtocol::GoogleGenerateContent
        && provider
            .google
            .as_ref()
            .is_some_and(|settings| settings.mode == Mode::Vertex)
}
pub(crate) fn normalize_settings(mut settings: Config) -> Config {
    for value in [
        &mut settings.project_id,
        &mut settings.location,
        &mut settings.quota_project_id,
    ] {
        *value = value.take().and_then(|text| {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_owned())
        });
    }
    settings
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    AiStudio,
    Vertex,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub mode: Mode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_project_id: Option<String>,
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        for value in [&self.project_id, &self.location, &self.quota_project_id]
            .into_iter()
            .flatten()
        {
            if value.is_empty()
                || value.len() > 128
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            {
                return Err("Google project/location identifiers must contain only letters, numbers, dots, underscores or hyphens".into());
            }
        }
        if self.mode == Mode::Vertex && (self.project_id.is_none() || self.location.is_none()) {
            return Err("Vertex requires google.project_id and google.location".into());
        }
        Ok(())
    }
    pub(crate) fn endpoint(&self, base: &str) -> Result<String, ProviderError> {
        self.validate().map_err(ProviderError::Protocol)?;
        let mut url = reqwest::Url::parse(base)
            .map_err(|_| ProviderError::Protocol("Google endpoint is invalid".into()))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(ProviderError::Protocol(
                "Google endpoint cannot contain credentials, query or fragment".into(),
            ));
        }
        if self.mode == Mode::Vertex {
            let location = self.location.as_deref().unwrap();
            let project = self.project_id.as_deref().unwrap();
            if url.host_str() == Some("aiplatform.googleapis.com") && location != "global" {
                url.set_host(Some(&format!("{location}-aiplatform.googleapis.com")))
                    .map_err(|_| ProviderError::Protocol("Vertex location is invalid".into()))?;
            }
            let suffix = format!("/projects/{project}/locations/{location}/publishers/google");
            if !url.path().trim_end_matches('/').ends_with(&suffix) {
                let path = format!("{}{}", url.path().trim_end_matches('/'), suffix);
                url.set_path(&path);
            }
        }
        Ok(url.to_string().trim_end_matches('/').to_string())
    }
}

#[derive(Clone)]
pub(crate) struct Auth {
    config: Config,
    credentials: Arc<tokio::sync::OnceCell<Credentials>>,
    identity: Arc<tokio::sync::OnceCell<String>>,
}
impl Auth {
    pub(crate) fn new(config: Config) -> Self {
        Self {
            config,
            credentials: Arc::new(tokio::sync::OnceCell::new()),
            identity: Arc::new(tokio::sync::OnceCell::new()),
        }
    }
    fn qualify_identity(&self, raw: &str) -> String {
        self.config.quota_project_id.as_ref().map_or_else(
            || raw.to_owned(),
            |quota| {
                format!(
                    "{:x}",
                    Sha256::digest(format!("ca-adc-quota-v1\0{raw}\0{quota}").as_bytes())
                )
            },
        )
    }
    pub(crate) fn cached_identity(&self) -> Option<String> {
        self.identity.get().map(|raw| self.qualify_identity(raw))
    }
    pub(crate) async fn headers(&self) -> Result<http::HeaderMap, ProviderError> {
        let config = self.config.clone();
        let credentials=self.credentials.get_or_try_init(||async move{
   tokio::task::spawn_blocking(move||{
    let mut builder=Builder::default().with_scopes(["https://www.googleapis.com/auth/cloud-platform"]);
    if let Some(project)=config.quota_project_id{builder=builder.with_quota_project_id(project);}
    builder.build().map_err(|_|ProviderError::Protocol("Vertex ADC credentials are unavailable; configure Application Default Credentials".into()))
   }).await.map_err(|_|ProviderError::Protocol("Vertex ADC credential worker failed".into()))?
  }).await?;
        match credentials
            .headers(http::Extensions::new())
            .await
            .map_err(|_| ProviderError::Protocol("Vertex ADC token refresh failed".into()))?
        {
            CacheableResource::New { data, .. } => Ok(data),
            CacheableResource::NotModified => Err(ProviderError::Protocol(
                "Vertex ADC returned no authentication headers".into(),
            )),
        }
    }
    pub(crate) async fn identity(&self) -> Result<String, ProviderError> {
        self.identity
            .get_or_try_init(|| async {
                tokio::task::spawn_blocking(|| {
                    let configured = std::env::var_os("GOOGLE_APPLICATION_CREDENTIALS")
                        .map(std::path::PathBuf::from);
                    let source = configured.clone().or_else(|| {
                        std::env::var_os("HOME").map(|home| {
                            std::path::PathBuf::from(home)
                                .join(".config/gcloud/application_default_credentials.json")
                        })
                    });
                    if let Some(path) = source {
                        match std::fs::metadata(&path) {
                            Ok(info) if info.is_file() && info.len() <= 1024 * 1024 => {
                                use std::io::Read;
                                let file = std::fs::File::open(path).map_err(|_| {
                                    ProviderError::Protocol(
                                        "Vertex ADC identity is unreadable".into(),
                                    )
                                })?;
                                if !file
                                    .metadata()
                                    .map_err(|_| {
                                        ProviderError::Protocol(
                                            "Vertex ADC identity is unreadable".into(),
                                        )
                                    })?
                                    .is_file()
                                {
                                    return Err(ProviderError::Protocol(
                                        "Vertex ADC identity source is not a file".into(),
                                    ));
                                }
                                let mut bytes = Vec::new();
                                file.take(1024 * 1024 + 1).read_to_end(&mut bytes).map_err(
                                    |_| {
                                        ProviderError::Protocol(
                                            "Vertex ADC identity is unreadable".into(),
                                        )
                                    },
                                )?;
                                if bytes.len() > 1024 * 1024 {
                                    return Err(ProviderError::Protocol(
                                        "Vertex ADC identity source exceeds its byte limit".into(),
                                    ));
                                }
                                return Ok(format!("{:x}", Sha256::digest(bytes)));
                            }
                            _ if configured.is_some() => {
                                return Err(ProviderError::Protocol(
                                    "Vertex ADC identity source is unavailable".into(),
                                ));
                            }
                            _ => {}
                        }
                    }
                    // Metadata credentials do not expose a stable account identity through the SDK.
                    // Keep their opaque continuation process-local instead of assuming identity across restarts.
                    {
                        static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
                        Ok(ID
                            .get_or_init(|| format!("metadata:{}", uuid::Uuid::new_v4()))
                            .clone())
                    }
                })
                .await
                .map_err(|_| ProviderError::Protocol("Vertex ADC identity worker failed".into()))?
            })
            .await
            .cloned()
            .map(|raw| self.qualify_identity(&raw))
    }
    #[cfg(test)]
    pub(crate) fn with_credentials(config: Config, credentials: Credentials) -> Self {
        let auth = Self::new(config);
        auth.credentials.set(credentials).expect("fresh ADC cell");
        auth.identity.set("mock-adc-account".into()).unwrap();
        auth
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use google_cloud_auth::credentials::{CredentialsProvider, EntityTag};
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Debug, Clone)]
    struct Mock(Arc<AtomicUsize>);
    impl CredentialsProvider for Mock {
        async fn universe_domain(&self) -> Option<String> {
            None
        }
        async fn headers(
            &self,
            _: http::Extensions,
        ) -> Result<CacheableResource<http::HeaderMap>, google_cloud_auth::errors::CredentialsError>
        {
            let generation = self.0.fetch_add(1, Ordering::SeqCst);
            let mut headers = http::HeaderMap::new();
            headers.insert(
                http::header::AUTHORIZATION,
                format!("Bearer fixture-{generation}").parse().unwrap(),
            );
            Ok(CacheableResource::New {
                entity_tag: EntityTag::new(),
                data: headers,
            })
        }
    }
    #[tokio::test]
    async fn google_adc_requests_fresh_headers_without_copying_tokens_to_config() {
        let count = Arc::new(AtomicUsize::new(0));
        let config = Config {
            mode: Mode::Vertex,
            project_id: Some("example-project".into()),
            location: Some("global".into()),
            quota_project_id: None,
        };
        let auth = Auth::with_credentials(config.clone(), Credentials::from(Mock(count.clone())));
        let one = auth.headers().await.unwrap();
        let two = auth.headers().await.unwrap();
        assert_ne!(
            one[http::header::AUTHORIZATION],
            two[http::header::AUTHORIZATION]
        );
        assert_eq!(auth.config, config);
        assert!(!serde_json::to_string(&auth.config)
            .unwrap()
            .contains("fixture-"));
        assert_eq!(auth.identity().await.unwrap(), "mock-adc-account");
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn google_vertex_regional_global_and_loopback_endpoints_are_exact() {
        let mut config = Config {
            mode: Mode::Vertex,
            project_id: Some("project".into()),
            location: Some("global".into()),
            quota_project_id: None,
        };
        assert_eq!(
            config
                .endpoint("https://aiplatform.googleapis.com/v1")
                .unwrap(),
            "https://aiplatform.googleapis.com/v1/projects/project/locations/global/publishers/google"
        );
        config.location = Some("us-central1".into());
        assert!(config
            .endpoint("https://aiplatform.googleapis.com/v1")
            .unwrap()
            .starts_with("https://us-central1-aiplatform.googleapis.com/"));
        assert!(config
            .endpoint("http://127.0.0.1:1234/v1")
            .unwrap()
            .starts_with("http://127.0.0.1:1234/"));
        config.project_id = Some("bad/project".into());
        assert!(config.validate().is_err());
    }
    #[derive(Debug, Clone)]
    struct FailedRefresh;
    impl CredentialsProvider for FailedRefresh {
        async fn universe_domain(&self) -> Option<String> {
            None
        }
        async fn headers(
            &self,
            _: http::Extensions,
        ) -> Result<CacheableResource<http::HeaderMap>, google_cloud_auth::errors::CredentialsError>
        {
            Err(google_cloud_auth::errors::CredentialsError::new(
                false,
                "fixture-private-token",
                std::io::Error::other("fixture-private-source"),
            ))
        }
    }
    #[tokio::test]
    async fn google_adc_refresh_failure_does_not_expose_sdk_details() {
        let auth = Auth::with_credentials(Config::default(), Credentials::from(FailedRefresh));
        let error = auth.headers().await.unwrap_err().to_string();
        assert!(error.contains("ADC token refresh failed"));
        assert!(!error.contains("fixture-private"));
    }
    #[test]
    fn google_adc_discovery_failure_uses_disposable_state() {
        let state = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "google_auth::tests::google_adc_discovery_child",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", state.path())
            .env(
                "GOOGLE_APPLICATION_CREDENTIALS",
                state.path().join("missing-fixture.json"),
            )
            .env("CA_GOOGLE_DISCOVERY_FIXTURE", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
    #[tokio::test]
    async fn google_adc_discovery_child() {
        if std::env::var("CA_GOOGLE_DISCOVERY_FIXTURE").as_deref() != Ok("1") {
            return;
        }
        let auth = Auth::new(Config::default());
        let error = auth.headers().await.unwrap_err().to_string();
        assert!(error.contains("ADC credentials are unavailable"));
        assert!(!error.contains("missing-fixture"));
    }
}
