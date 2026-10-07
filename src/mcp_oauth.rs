use async_trait::async_trait;
use rmcp::transport::auth::{AuthError, CredentialStore, StoredCredentials};
use rmcp::transport::auth::{
    AuthorizationManager, AuthorizationRequest, AuthorizationSession, OAuthHttpClient,
    OAuthHttpClientError, OAuthHttpClientFuture, OAuthHttpRequest,
};
use std::sync::Arc;

#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "ca.mcp-oauth";
const MAX_CREDENTIAL_BYTES: usize = 64 * 1024;
const MAX_OAUTH_REQUEST_BYTES: usize = 64 * 1024;
const MAX_OAUTH_RESPONSE_BYTES: usize = 1024 * 1024;

pub trait McpOAuthSecretStore: Send + Sync {
    fn load(&self, account: &str) -> anyhow::Result<Option<Vec<u8>>>;
    fn store(&self, account: &str, secret: &[u8]) -> anyhow::Result<()>;
    fn delete(&self, account: &str) -> anyhow::Result<()>;
}

pub fn system_mcp_oauth_secret_store() -> Arc<dyn McpOAuthSecretStore> {
    Arc::new(SystemMcpOAuthSecretStore)
}

pub fn secret_store_for_profile(
    profile: &crate::runtime::ActiveProfile,
) -> Arc<dyn McpOAuthSecretStore> {
    if matches!(profile, crate::runtime::ActiveProfile::Bare) {
        Arc::new(InMemoryMcpOAuthSecretStore::default())
    } else {
        system_mcp_oauth_secret_store()
    }
}

#[derive(Default)]
struct InMemoryMcpOAuthSecretStore(std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>);

impl McpOAuthSecretStore for InMemoryMcpOAuthSecretStore {
    fn load(&self, account: &str) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(account)
            .cloned())
    }

    fn store(&self, account: &str, secret: &[u8]) -> anyhow::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(account.into(), secret.into());
        Ok(())
    }

    fn delete(&self, account: &str) -> anyhow::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(account);
        Ok(())
    }
}

pub trait BrowserOpener: Send + Sync {
    fn open(&self, authorization_url: &str) -> anyhow::Result<()>;
}

pub fn system_browser_opener() -> Arc<dyn BrowserOpener> {
    Arc::new(SystemBrowserOpener)
}

struct SystemBrowserOpener;

impl BrowserOpener for SystemBrowserOpener {
    fn open(&self, authorization_url: &str) -> anyhow::Result<()> {
        #[cfg(target_os = "macos")]
        {
            let status = std::process::Command::new("/usr/bin/open")
                .arg(authorization_url)
                .status()?;
            anyhow::ensure!(status.success(), "browser did not open");
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = authorization_url;
            anyhow::bail!("automatic browser opening is unavailable")
        }
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum McpOAuthActionError {
    #[error("MCP authorization discovery failed")]
    Discovery,
    #[error("MCP authorization could not open a browser")]
    Browser,
    #[error("MCP authorization callback was invalid")]
    Callback,
    #[error("MCP authorization timed out")]
    TimedOut,
    #[error("MCP authorization was rejected")]
    Rejected,
    #[error("MCP device authorization is not supported by this server")]
    DeviceUnsupported,
    #[error("MCP authorization credential storage failed")]
    Storage,
}

#[derive(Clone, PartialEq, Eq)]
pub enum McpOAuthProgress {
    OpeningBrowser,
    DeviceCode {
        verification_host: String,
        user_code: String,
    },
    WaitingForAuthorization,
}

impl std::fmt::Debug for McpOAuthProgress {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpeningBrowser => formatter.write_str("OpeningBrowser"),
            Self::DeviceCode {
                verification_host, ..
            } => formatter
                .debug_struct("DeviceCode")
                .field("verification_host", verification_host)
                .field("user_code", &"[REDACTED]")
                .finish(),
            Self::WaitingForAuthorization => formatter.write_str("WaitingForAuthorization"),
        }
    }
}

impl McpOAuthProgress {
    pub fn render(&self) -> String {
        match self {
            Self::OpeningBrowser => "Opening the MCP authorization page…".into(),
            Self::DeviceCode {
                verification_host,
                user_code,
            } => format!("Open {verification_host} and enter code {user_code}"),
            Self::WaitingForAuthorization => "Waiting for MCP authorization… (Esc cancels)".into(),
        }
    }
}

pub type ProgressSink = Arc<dyn Fn(McpOAuthProgress) + Send + Sync>;

pub fn credential_account(profile: &crate::runtime::ActiveProfile, server: &str) -> String {
    match profile {
        crate::runtime::ActiveProfile::Named(profile) => {
            format!("profile/{profile}/server/{server}")
        }
        crate::runtime::ActiveProfile::Default | crate::runtime::ActiveProfile::Bare => {
            format!("server/{server}")
        }
    }
}

#[derive(Clone)]
pub struct KeychainCredentialStore {
    store: Arc<dyn McpOAuthSecretStore>,
    account: Arc<str>,
}

impl std::fmt::Debug for KeychainCredentialStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KeychainCredentialStore")
            .finish_non_exhaustive()
    }
}

impl KeychainCredentialStore {
    pub fn new(
        store: Arc<dyn McpOAuthSecretStore>,
        profile: &crate::runtime::ActiveProfile,
        server: &str,
    ) -> Self {
        Self {
            store,
            account: credential_account(profile, server).into(),
        }
    }

    fn safe_store_error() -> AuthError {
        AuthError::InternalError("secure credential storage failed".into())
    }

    fn decode(bytes: &[u8]) -> Result<StoredCredentials, AuthError> {
        if bytes.len() > MAX_CREDENTIAL_BYTES {
            return Err(Self::safe_store_error());
        }
        serde_json::from_slice(bytes).map_err(|_| Self::safe_store_error())
    }
}

#[async_trait]
impl CredentialStore for KeychainCredentialStore {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        let store = self.store.clone();
        let account = self.account.clone();
        let bytes = tokio::task::spawn_blocking(move || store.load(&account))
            .await
            .map_err(|_| Self::safe_store_error())?
            .map_err(|_| Self::safe_store_error())?;
        bytes.as_deref().map(Self::decode).transpose()
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let encoded = serde_json::to_vec(&credentials).map_err(|_| Self::safe_store_error())?;
        if encoded.len() > MAX_CREDENTIAL_BYTES {
            return Err(Self::safe_store_error());
        }
        let store = self.store.clone();
        let account = self.account.clone();
        tokio::task::spawn_blocking(move || store.store(&account, &encoded))
            .await
            .map_err(|_| Self::safe_store_error())?
            .map_err(|_| Self::safe_store_error())
    }

    async fn clear(&self) -> Result<(), AuthError> {
        let store = self.store.clone();
        let account = self.account.clone();
        tokio::task::spawn_blocking(move || store.delete(&account))
            .await
            .map_err(|_| Self::safe_store_error())?
            .map_err(|_| Self::safe_store_error())
    }
}

struct SystemMcpOAuthSecretStore;

#[cfg(target_os = "macos")]
impl McpOAuthSecretStore for SystemMcpOAuthSecretStore {
    fn load(&self, account: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let options = security_framework::passwords::PasswordOptions::new_generic_password(
            KEYCHAIN_SERVICE,
            account,
        );
        match security_framework::passwords::generic_password(options) {
            Ok(secret) => Ok(Some(secret)),
            Err(error) if error.code() == -25_300 => Ok(None),
            Err(_) => anyhow::bail!("macOS Keychain could not read MCP authorization"),
        }
    }

    fn store(&self, account: &str, secret: &[u8]) -> anyhow::Result<()> {
        security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, account, secret)
            .map_err(|_| anyhow::anyhow!("macOS Keychain could not save MCP authorization"))
    }

    fn delete(&self, account: &str) -> anyhow::Result<()> {
        match security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, account) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == -25_300 => Ok(()),
            Err(_) => anyhow::bail!("macOS Keychain could not delete MCP authorization"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("MCP authorization request failed")]
struct SafeOAuthHttpError;

#[derive(Clone)]
pub struct PinnedOAuthHttpClient {
    client: reqwest_13::Client,
    resource_origin: Origin,
    issuer_origin: Option<Origin>,
}

impl std::fmt::Debug for PinnedOAuthHttpClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PinnedOAuthHttpClient")
            .field("issuer_pinned", &self.issuer_origin.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Origin {
    scheme: String,
    host: String,
    port: u16,
}

impl Origin {
    fn from_url(url: &reqwest_13::Url) -> Result<Self, AuthError> {
        Ok(Self {
            scheme: url.scheme().into(),
            host: url
                .host_str()
                .ok_or_else(|| AuthError::InternalError("invalid authorization authority".into()))?
                .to_ascii_lowercase(),
            port: url.port_or_known_default().ok_or_else(|| {
                AuthError::InternalError("invalid authorization authority".into())
            })?,
        })
    }
}

impl PinnedOAuthHttpClient {
    pub fn new(
        resource: &str,
        issuer: Option<&str>,
        timeout: std::time::Duration,
    ) -> Result<Self, AuthError> {
        let resource = reqwest_13::Url::parse(resource)
            .map_err(|_| AuthError::InternalError("invalid authorization resource".into()))?;
        let issuer = issuer
            .map(reqwest_13::Url::parse)
            .transpose()
            .map_err(|_| AuthError::InternalError("invalid authorization issuer".into()))?;
        let client = reqwest_13::Client::builder()
            .connect_timeout(timeout)
            .timeout(timeout)
            .redirect(reqwest_13::redirect::Policy::none())
            .no_proxy()
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|_| AuthError::InternalError("authorization client setup failed".into()))?;
        Ok(Self {
            client,
            resource_origin: Origin::from_url(&resource)?,
            issuer_origin: issuer.as_ref().map(Origin::from_url).transpose()?,
        })
    }

    fn allows(&self, url: &reqwest_13::Url) -> bool {
        let Ok(origin) = Origin::from_url(url) else {
            return false;
        };
        if origin != self.resource_origin
            && self
                .issuer_origin
                .as_ref()
                .is_none_or(|issuer| issuer != &origin)
        {
            return false;
        }
        origin.scheme == "https"
            || (origin.scheme == "http"
                && self.resource_origin.scheme == "http"
                && origin
                    .host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
                && self
                    .resource_origin
                    .host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback()))
    }

    async fn post_form(
        &self,
        endpoint: &str,
        fields: &[(String, String)],
    ) -> Result<BoundedOAuthResponse, McpOAuthActionError> {
        let endpoint =
            reqwest_13::Url::parse(endpoint).map_err(|_| McpOAuthActionError::Discovery)?;
        if !self.allows(&endpoint) {
            return Err(McpOAuthActionError::Discovery);
        }
        let response = self
            .client
            .post(endpoint)
            .header(http::header::ACCEPT, "application/json")
            .form(fields)
            .send()
            .await
            .map_err(|_| McpOAuthActionError::Discovery)?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > MAX_OAUTH_RESPONSE_BYTES as u64)
        {
            return Err(McpOAuthActionError::Discovery);
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        use futures::StreamExt as _;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| McpOAuthActionError::Discovery)?;
            if body.len().saturating_add(chunk.len()) > MAX_OAUTH_RESPONSE_BYTES {
                return Err(McpOAuthActionError::Discovery);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(BoundedOAuthResponse { status, body })
    }
}

struct BoundedOAuthResponse {
    status: reqwest_13::StatusCode,
    body: Vec<u8>,
}

impl OAuthHttpClient for PinnedOAuthHttpClient {
    fn execute(&self, request: OAuthHttpRequest) -> OAuthHttpClientFuture<'_> {
        Box::pin(async move {
            let request = request.request;
            let (parts, body) = request.into_parts();
            if body.len() > MAX_OAUTH_REQUEST_BYTES {
                return Err(Box::new(SafeOAuthHttpError) as _);
            }
            let url = reqwest_13::Url::parse(&parts.uri.to_string())
                .map_err(|_| Box::new(SafeOAuthHttpError) as OAuthHttpClientError)?;
            if !self.allows(&url) {
                return Err(Box::new(SafeOAuthHttpError) as _);
            }
            let mut outgoing = self.client.request(parts.method, url).body(body);
            for (name, value) in &parts.headers {
                outgoing = outgoing.header(name, value);
            }
            let response = outgoing
                .send()
                .await
                .map_err(|_| Box::new(SafeOAuthHttpError) as OAuthHttpClientError)?;
            let mut builder = oauth2::http::Response::builder()
                .status(response.status())
                .version(response.version());
            for (name, value) in response.headers() {
                builder = builder.header(name, value);
            }
            let mut bytes = Vec::new();
            let mut stream = response.bytes_stream();
            use futures::StreamExt as _;
            while let Some(chunk) = stream.next().await {
                let chunk =
                    chunk.map_err(|_| Box::new(SafeOAuthHttpError) as OAuthHttpClientError)?;
                if bytes.len().saturating_add(chunk.len()) > MAX_OAUTH_RESPONSE_BYTES {
                    return Err(Box::new(SafeOAuthHttpError) as _);
                }
                bytes.extend_from_slice(&chunk);
            }
            builder
                .body(bytes)
                .map_err(|_| Box::new(SafeOAuthHttpError) as _)
        })
    }
}

pub async fn initialized_authorization_manager(
    config: &crate::config::McpServerConfig,
    profile: &crate::runtime::ActiveProfile,
    server: &str,
    store: Arc<dyn McpOAuthSecretStore>,
) -> Result<(AuthorizationManager, bool), AuthError> {
    let configured_client_secret = config
        .oauth
        .client_secret
        .as_deref()
        .map(|reference| {
            resolve_environment_reference(reference).ok_or_else(|| {
                AuthError::InternalError("MCP OAuth client secret is unavailable".into())
            })
        })
        .transpose()?;
    let resource = config
        .url
        .as_deref()
        .ok_or_else(|| AuthError::InternalError("missing authorization resource".into()))?;
    let http = Arc::new(PinnedOAuthHttpClient::new(
        resource,
        config.oauth.issuer.as_deref(),
        std::time::Duration::from_secs(config.timeout_secs),
    )?);
    let mut manager = AuthorizationManager::new_with_oauth_http_client(resource, http).await?;
    let credentials = KeychainCredentialStore::new(store, profile, server);
    manager.set_credential_store(credentials.clone());
    let resolution = manager.resolve_metadata().await?;
    if !resolution.source.is_discovered() {
        return Err(AuthError::NoAuthorizationSupport);
    }
    validate_metadata_binding(config, &resolution.metadata)?;
    manager.set_metadata(resolution.metadata);
    if let Some(stored) = CredentialStore::load(&credentials).await? {
        let expected_client = config
            .oauth
            .client_id
            .as_deref()
            .or(config.oauth.client_metadata_url.as_deref());
        if expected_client.is_some_and(|expected| expected != stored.client_id) {
            CredentialStore::clear(&credentials).await?;
        }
    }
    let mut initialized = manager.initialize_from_store().await?;
    if initialized {
        if let (Some(client_id), Some(secret)) =
            (config.oauth.client_id.as_deref(), configured_client_secret)
        {
            manager.configure_client(
                rmcp::transport::auth::OAuthClientConfig::new(
                    client_id,
                    "http://127.0.0.1/oauth/callback",
                )
                .with_client_secret(secret)
                .with_scopes(config.oauth.scopes.clone()),
            )?;
        }
        match manager.get_access_token().await {
            Ok(_) => {}
            Err(AuthError::AuthorizationRequired) => {
                CredentialStore::clear(&credentials).await?;
                initialized = false;
            }
            Err(error) => return Err(error),
        }
    }
    Ok((manager, initialized))
}

pub async fn authorize_browser(
    config: &crate::config::McpServerConfig,
    profile: &crate::runtime::ActiveProfile,
    server: &str,
    store: Arc<dyn McpOAuthSecretStore>,
    opener: Arc<dyn BrowserOpener>,
    progress: Option<ProgressSink>,
) -> Result<(), McpOAuthActionError> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| McpOAuthActionError::Callback)?;
    let port = listener
        .local_addr()
        .map_err(|_| McpOAuthActionError::Callback)?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/oauth/callback");
    let (manager, _) = initialized_authorization_manager(config, profile, server, store)
        .await
        .map_err(|_| McpOAuthActionError::Discovery)?;
    let mut request = AuthorizationRequest::new(&redirect_uri)
        .with_scopes(config.oauth.scopes.clone())
        .with_client_name("ca MCP client");
    if let Some(client_id) = config.oauth.client_id.as_deref() {
        request = request.with_preregistered_client(client_id);
    }
    if let Some(reference) = config.oauth.client_secret.as_deref() {
        request = request.with_client_secret(
            resolve_environment_reference(reference).ok_or(McpOAuthActionError::Discovery)?,
        );
    }
    if let Some(metadata_url) = config.oauth.client_metadata_url.as_deref() {
        request = request.with_client_metadata_url(metadata_url);
    }
    let session = AuthorizationSession::new(manager, request)
        .await
        .map_err(|_| McpOAuthActionError::Discovery)?;
    let authorization_url = session.get_authorization_url().to_string();
    let allowed = PinnedOAuthHttpClient::new(
        config.url.as_deref().unwrap_or_default(),
        config.oauth.issuer.as_deref(),
        std::time::Duration::from_secs(config.timeout_secs),
    )
    .map_err(|_| McpOAuthActionError::Discovery)?;
    let parsed =
        reqwest_13::Url::parse(&authorization_url).map_err(|_| McpOAuthActionError::Discovery)?;
    if !allowed.allows(&parsed) {
        return Err(McpOAuthActionError::Discovery);
    }
    if let Some(progress) = &progress {
        progress(McpOAuthProgress::OpeningBrowser);
    }
    tokio::task::spawn_blocking(move || opener.open(&authorization_url))
        .await
        .map_err(|_| McpOAuthActionError::Browser)?
        .map_err(|_| McpOAuthActionError::Browser)?;

    if let Some(progress) = &progress {
        progress(McpOAuthProgress::WaitingForAuthorization);
    }

    let callback = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        receive_callback(listener, &redirect_uri),
    )
    .await
    .map_err(|_| McpOAuthActionError::TimedOut)??;
    let result = session.handle_callback_url(&callback).await;
    if result.is_err() {
        return Err(McpOAuthActionError::Rejected);
    }
    Ok(())
}

#[derive(serde::Deserialize)]
struct DeviceAuthorizationResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    expires_in: u64,
    #[serde(default = "default_device_interval")]
    interval: u64,
}

fn default_device_interval() -> u64 {
    5
}

pub async fn authorize_device(
    config: &crate::config::McpServerConfig,
    profile: &crate::runtime::ActiveProfile,
    server: &str,
    store: Arc<dyn McpOAuthSecretStore>,
    opener: Arc<dyn BrowserOpener>,
    progress: Option<ProgressSink>,
) -> Result<(), McpOAuthActionError> {
    let resource = config
        .url
        .as_deref()
        .ok_or(McpOAuthActionError::Discovery)?;
    let client = PinnedOAuthHttpClient::new(
        resource,
        config.oauth.issuer.as_deref(),
        std::time::Duration::from_secs(config.timeout_secs),
    )
    .map_err(|_| McpOAuthActionError::Discovery)?;
    let mut manager =
        AuthorizationManager::new_with_oauth_http_client(resource, Arc::new(client.clone()))
            .await
            .map_err(|_| McpOAuthActionError::Discovery)?;
    let credentials = KeychainCredentialStore::new(store, profile, server);
    manager.set_credential_store(credentials.clone());
    let resolution = manager
        .resolve_metadata()
        .await
        .map_err(|_| McpOAuthActionError::Discovery)?;
    if !resolution.source.is_discovered() {
        return Err(McpOAuthActionError::DeviceUnsupported);
    }
    validate_metadata_binding(config, &resolution.metadata)
        .map_err(|_| McpOAuthActionError::Discovery)?;
    let device_endpoint = resolution
        .metadata
        .additional_fields
        .get("device_authorization_endpoint")
        .and_then(serde_json::Value::as_str)
        .ok_or(McpOAuthActionError::DeviceUnsupported)?;
    let client_id = config
        .oauth
        .client_id
        .as_deref()
        .ok_or(McpOAuthActionError::DeviceUnsupported)?;
    let mut fields = vec![
        ("client_id".into(), client_id.into()),
        ("resource".into(), resource.into()),
    ];
    if !config.oauth.scopes.is_empty() {
        fields.push(("scope".into(), config.oauth.scopes.join(" ")));
    }
    let response = client.post_form(device_endpoint, &fields).await?;
    if !response.status.is_success() {
        return Err(McpOAuthActionError::Rejected);
    }
    let device: DeviceAuthorizationResponse =
        serde_json::from_slice(&response.body).map_err(|_| McpOAuthActionError::Rejected)?;
    if device.device_code.is_empty()
        || device.device_code.len() > 4096
        || device.user_code.is_empty()
        || device.user_code.len() > 64
        || device.user_code.chars().any(char::is_control)
        || device.expires_in == 0
        || device.expires_in > 600
    {
        return Err(McpOAuthActionError::Rejected);
    }
    let verification = reqwest_13::Url::parse(&device.verification_uri)
        .map_err(|_| McpOAuthActionError::Rejected)?;
    if !client.allows(&verification) {
        return Err(McpOAuthActionError::Rejected);
    }
    let complete = device
        .verification_uri_complete
        .as_deref()
        .and_then(|value| reqwest_13::Url::parse(value).ok())
        .filter(|url| client.allows(url));
    let verification_host = verification
        .host_str()
        .filter(|host| host.len() <= 253 && host.is_ascii())
        .ok_or(McpOAuthActionError::Rejected)?
        .to_string();
    if let Some(progress) = &progress {
        progress(McpOAuthProgress::DeviceCode {
            verification_host,
            user_code: device.user_code.clone(),
        });
    }
    let open_url = complete
        .as_ref()
        .map_or(device.verification_uri.as_str(), reqwest_13::Url::as_str)
        .to_string();
    tokio::task::spawn_blocking(move || opener.open(&open_url))
        .await
        .map_err(|_| McpOAuthActionError::Browser)?
        .map_err(|_| McpOAuthActionError::Browser)?;
    if let Some(progress) = &progress {
        progress(McpOAuthProgress::WaitingForAuthorization);
    }

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(device.expires_in);
    let mut interval = device.interval.clamp(1, 15);
    let token = loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(McpOAuthActionError::TimedOut);
        }
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
        let response = client
            .post_form(
                &resolution.metadata.token_endpoint,
                &[
                    (
                        "grant_type".into(),
                        "urn:ietf:params:oauth:grant-type:device_code".into(),
                    ),
                    ("device_code".into(), device.device_code.clone()),
                    ("client_id".into(), client_id.into()),
                    ("resource".into(), resource.into()),
                ],
            )
            .await?;
        if response.status.is_success() {
            break serde_json::from_slice::<rmcp::transport::auth::OAuthTokenResponse>(
                &response.body,
            )
            .map_err(|_| McpOAuthActionError::Rejected)?;
        }
        let error = serde_json::from_slice::<serde_json::Value>(&response.body)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            });
        match device_poll_action(error.as_deref()) {
            DevicePollAction::Continue => {}
            DevicePollAction::SlowDown => interval = interval.saturating_add(5).min(30),
            DevicePollAction::Rejected => return Err(McpOAuthActionError::Rejected),
            DevicePollAction::Expired => return Err(McpOAuthActionError::TimedOut),
        }
    };
    let issuer = resolution.metadata.issuer.clone();
    let received_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| McpOAuthActionError::Storage)?
        .as_secs();
    let stored = StoredCredentials::new(
        client_id.into(),
        Some(token),
        config.oauth.scopes.clone(),
        Some(received_at),
    )
    .with_issuer(issuer);
    CredentialStore::save(&credentials, stored)
        .await
        .map_err(|_| McpOAuthActionError::Storage)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DevicePollAction {
    Continue,
    SlowDown,
    Rejected,
    Expired,
}

fn device_poll_action(error: Option<&str>) -> DevicePollAction {
    match error {
        Some("authorization_pending") => DevicePollAction::Continue,
        Some("slow_down") => DevicePollAction::SlowDown,
        Some("expired_token") => DevicePollAction::Expired,
        _ => DevicePollAction::Rejected,
    }
}

async fn receive_callback(
    listener: tokio::net::TcpListener,
    redirect_uri: &str,
) -> Result<String, McpOAuthActionError> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let (mut stream, peer) = listener
        .accept()
        .await
        .map_err(|_| McpOAuthActionError::Callback)?;
    if !peer.ip().is_loopback() {
        return Err(McpOAuthActionError::Callback);
    }
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 2048];
        let count = stream
            .read(&mut chunk)
            .await
            .map_err(|_| McpOAuthActionError::Callback)?;
        if count == 0 || bytes.len().saturating_add(count) > 16 * 1024 {
            return Err(McpOAuthActionError::Callback);
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers =
        std::str::from_utf8(&bytes[..header_end]).map_err(|_| McpOAuthActionError::Callback)?;
    let mut lines = headers.split("\r\n");
    let request_line = lines.next().ok_or(McpOAuthActionError::Callback)?;
    let mut request = request_line.split_whitespace();
    if request.next() != Some("GET") {
        return Err(McpOAuthActionError::Callback);
    }
    let target = request.next().ok_or(McpOAuthActionError::Callback)?;
    if request.next() != Some("HTTP/1.1") || !target.starts_with("/oauth/callback?") {
        return Err(McpOAuthActionError::Callback);
    }
    let expected_host = reqwest_13::Url::parse(redirect_uri)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| format!("{host}:{}", url.port_or_known_default().unwrap_or(443)))
        })
        .ok_or(McpOAuthActionError::Callback)?;
    let host = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.trim());
    if host != Some(expected_host.as_str()) {
        return Err(McpOAuthActionError::Callback);
    }
    let callback = format!("http://{expected_host}{target}");
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 36\r\nConnection: close\r\n\r\nAuthorization complete. Return to ca.",
        )
        .await
        .map_err(|_| McpOAuthActionError::Callback)?;
    Ok(callback)
}

fn resolve_environment_reference(reference: &str) -> Option<String> {
    let name = reference.strip_prefix("${")?.strip_suffix('}')?;
    if name.is_empty()
        || name.len() > 128
        || name.as_bytes()[0].is_ascii_digit()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return None;
    }
    crate::run_environment::var(name)
}

fn validate_metadata_binding(
    config: &crate::config::McpServerConfig,
    metadata: &rmcp::transport::auth::AuthorizationMetadata,
) -> Result<(), AuthError> {
    let resource = reqwest_13::Url::parse(config.url.as_deref().unwrap_or_default())
        .map_err(|_| AuthError::InternalError("invalid authorization resource".into()))?;
    let expected = config
        .oauth
        .issuer
        .as_deref()
        .map(reqwest_13::Url::parse)
        .transpose()
        .map_err(|_| AuthError::InternalError("invalid authorization issuer".into()))?;
    let issuer = metadata
        .issuer
        .as_deref()
        .ok_or(AuthError::AuthorizationServerMissingIssuer {
            expected_issuer: "configured issuer".into(),
        })
        .and_then(|value| {
            reqwest_13::Url::parse(value)
                .map_err(|_| AuthError::InternalError("invalid discovered issuer".into()))
        })?;
    if let Some(expected) = expected.as_ref() {
        if canonical_issuer(expected) != canonical_issuer(&issuer) {
            return Err(AuthError::AuthorizationServerMismatch {
                expected_issuer: "configured issuer".into(),
                received_issuer: "discovered issuer".into(),
            });
        }
    } else if Origin::from_url(&resource)? != Origin::from_url(&issuer)? {
        return Err(AuthError::AuthorizationServerMismatch {
            expected_issuer: "resource origin".into(),
            received_issuer: "cross-origin issuer".into(),
        });
    }
    for endpoint in [&metadata.authorization_endpoint, &metadata.token_endpoint] {
        let endpoint = reqwest_13::Url::parse(endpoint)
            .map_err(|_| AuthError::InternalError("invalid authorization endpoint".into()))?;
        if Origin::from_url(&endpoint)? != Origin::from_url(&issuer)? {
            return Err(AuthError::AuthorizationServerMismatch {
                expected_issuer: "issuer origin".into(),
                received_issuer: "endpoint origin".into(),
            });
        }
    }
    Ok(())
}

fn canonical_issuer(url: &reqwest_13::Url) -> String {
    url.as_str().trim_end_matches('/').to_string()
}

#[cfg(not(target_os = "macos"))]
impl McpOAuthSecretStore for SystemMcpOAuthSecretStore {
    fn load(&self, _account: &str) -> anyhow::Result<Option<Vec<u8>>> {
        anyhow::bail!("saved MCP authorization requires macOS Keychain")
    }

    fn store(&self, _account: &str, _secret: &[u8]) -> anyhow::Result<()> {
        anyhow::bail!("saved MCP authorization requires macOS Keychain")
    }

    fn delete(&self, _account: &str) -> anyhow::Result<()> {
        anyhow::bail!("saved MCP authorization requires macOS Keychain")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::transport::auth::OAuthTokenResponse;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    #[derive(Default)]
    struct FakeStore(Mutex<HashMap<String, Vec<u8>>>);

    impl McpOAuthSecretStore for FakeStore {
        fn load(&self, account: &str) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self.0.lock().unwrap().get(account).cloned())
        }
        fn store(&self, account: &str, secret: &[u8]) -> anyhow::Result<()> {
            self.0.lock().unwrap().insert(account.into(), secret.into());
            Ok(())
        }
        fn delete(&self, account: &str) -> anyhow::Result<()> {
            self.0.lock().unwrap().remove(account);
            Ok(())
        }
    }

    fn credentials(value: &str) -> StoredCredentials {
        let token: OAuthTokenResponse = serde_json::from_value(serde_json::json!({
            "access_token": value,
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": format!("refresh-{value}"),
            "scope": "tools"
        }))
        .unwrap();
        StoredCredentials::new(
            "public-client".into(),
            Some(token),
            vec!["tools".into()],
            Some(1),
        )
        .with_issuer(Some("https://issuer.invalid".into()))
    }

    #[tokio::test]
    async fn credentials_are_profile_scoped_redacted_and_idempotently_cleared() {
        let fake = Arc::new(FakeStore::default());
        let work = KeychainCredentialStore::new(
            fake.clone(),
            &crate::runtime::ActiveProfile::Named("work".into()),
            "docs",
        );
        let personal = KeychainCredentialStore::new(
            fake.clone(),
            &crate::runtime::ActiveProfile::Named("personal".into()),
            "docs",
        );
        work.save(credentials("top-secret")).await.unwrap();
        assert!(personal.load().await.unwrap().is_none());
        assert_eq!(
            work.load().await.unwrap().unwrap().client_id,
            "public-client"
        );
        assert!(!format!("{work:?}").contains("top-secret"));
        work.clear().await.unwrap();
        work.clear().await.unwrap();
        assert!(work.load().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn bare_credentials_remain_process_only() {
        let fake = Arc::new(InMemoryMcpOAuthSecretStore::default());
        let bare = KeychainCredentialStore::new(
            fake.clone(),
            &crate::runtime::ActiveProfile::Bare,
            "docs",
        );
        bare.save(credentials("volatile-secret")).await.unwrap();
        assert!(bare.load().await.unwrap().is_some());
        bare.clear().await.unwrap();
        assert!(bare.load().await.unwrap().is_none());
    }

    struct CallbackOpener;

    impl BrowserOpener for CallbackOpener {
        fn open(&self, authorization_url: &str) -> anyhow::Result<()> {
            let authorization = reqwest_13::Url::parse(authorization_url)?;
            let values = authorization
                .query_pairs()
                .map(|(name, value)| (name.into_owned(), value.into_owned()))
                .collect::<HashMap<_, _>>();
            let redirect = reqwest_13::Url::parse(&values["redirect_uri"])?;
            let state = values["state"].clone();
            std::thread::spawn(move || {
                let mut callback = redirect.clone();
                callback
                    .query_pairs_mut()
                    .append_pair("code", "fixture-code")
                    .append_pair("state", &state)
                    .append_pair("iss", authorization.origin().ascii_serialization().as_str());
                let address = format!(
                    "{}:{}",
                    redirect.host_str().unwrap(),
                    redirect.port().unwrap()
                );
                let mut stream = std::net::TcpStream::connect(address).unwrap();
                let target = format!("{}?{}", callback.path(), callback.query().unwrap());
                let host = format!(
                    "{}:{}",
                    redirect.host_str().unwrap(),
                    redirect.port().unwrap()
                );
                use std::io::{Read as _, Write as _};
                stream
                    .write_all(
                        format!(
                            "GET {target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    )
                    .unwrap();
                let mut response = String::new();
                stream.read_to_string(&mut response).unwrap();
                assert!(response.contains("Authorization complete"));
            });
            Ok(())
        }
    }

    struct NoCallbackOpener;

    impl BrowserOpener for NoCallbackOpener {
        fn open(&self, _authorization_url: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    async fn read_request(stream: &mut tokio::net::TcpStream) -> (String, String, Vec<u8>) {
        let mut bytes = Vec::new();
        let header_end = loop {
            let mut chunk = [0_u8; 2048];
            let count = stream.read(&mut chunk).await.unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
            if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
            assert!(bytes.len() <= 64 * 1024);
        };
        let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
        let mut lines = headers.split("\r\n");
        let mut request = lines.next().unwrap().split_whitespace();
        let method = request.next().unwrap().to_string();
        let path = request.next().unwrap().to_string();
        let content_length = lines
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        while bytes.len() - header_end < content_length {
            let mut chunk = [0_u8; 2048];
            let count = stream.read(&mut chunk).await.unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
        }
        (
            method,
            path,
            bytes[header_end..header_end + content_length].to_vec(),
        )
    }

    async fn write_json(
        stream: &mut tokio::net::TcpStream,
        status: &str,
        value: serde_json::Value,
    ) {
        let body = serde_json::to_vec(&value).unwrap();
        stream
            .write_all(
                format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream.write_all(&body).await.unwrap();
    }

    async fn start_oauth_fixture() -> (
        String,
        tokio::task::JoinHandle<Vec<(String, String, Vec<u8>)>>,
    ) {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let fixture_base = base.clone();
        let handle = tokio::spawn(async move {
            let mut requests = Vec::new();
            loop {
                let Ok(accepted) =
                    tokio::time::timeout(std::time::Duration::from_millis(500), listener.accept())
                        .await
                else {
                    break;
                };
                let (mut stream, _) = accepted.unwrap();
                let request = read_request(&mut stream).await;
                let path = request.1.clone();
                if path.contains("oauth-protected-resource") {
                    write_json(
                        &mut stream,
                        "200 OK",
                        serde_json::json!({
                            "resource": format!("{fixture_base}/mcp"),
                            "authorization_servers": [fixture_base]
                        }),
                    )
                    .await;
                } else if path.contains("oauth-authorization-server")
                    || path.contains("openid-configuration")
                {
                    write_json(
                        &mut stream,
                        "200 OK",
                        serde_json::json!({
                            "issuer": fixture_base,
                            "authorization_endpoint": format!("{fixture_base}/authorize"),
                            "token_endpoint": format!("{fixture_base}/token"),
                            "code_challenge_methods_supported": ["S256"]
                        }),
                    )
                    .await;
                } else if path == "/token" {
                    write_json(
                        &mut stream,
                        "200 OK",
                        serde_json::json!({
                            "access_token": "access-sentinel",
                            "token_type": "Bearer",
                            "expires_in": 3600,
                            "refresh_token": "refresh-sentinel",
                            "scope": "tools"
                        }),
                    )
                    .await;
                } else {
                    write_json(&mut stream, "404 Not Found", serde_json::json!({})).await;
                }
                requests.push(request);
                if requests.iter().any(|(_, path, _)| path == "/token") {
                    break;
                }
            }
            requests
        });
        (base, handle)
    }

    #[tokio::test]
    async fn browser_authorization_uses_pkce_loopback_callback_and_persists_safely() {
        let (base, fixture) = start_oauth_fixture().await;
        let store = Arc::new(FakeStore::default());
        let config = crate::config::McpServerConfig {
            transport: crate::config::McpTransport::StreamableHttp,
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            url: Some(format!("{base}/mcp")),
            headers: HashMap::new(),
            enabled: true,
            timeout_secs: 2,
            network: true,
            oauth: crate::config::McpOAuthConfig {
                enabled: true,
                issuer: Some(base.clone()),
                client_id: Some("fixture-client".into()),
                scopes: vec!["tools".into()],
                ..Default::default()
            },
        };
        authorize_browser(
            &config,
            &crate::runtime::ActiveProfile::Named("work".into()),
            "docs",
            store.clone(),
            Arc::new(CallbackOpener),
            None,
        )
        .await
        .unwrap();
        let requests = fixture.await.unwrap();
        let token_body = requests
            .iter()
            .find(|(_, path, _)| path == "/token")
            .map(|(_, _, body)| String::from_utf8_lossy(body).into_owned())
            .unwrap();
        assert!(token_body.contains("code_verifier="));
        assert!(token_body.contains("resource="));
        assert!(store
            .0
            .lock()
            .unwrap()
            .contains_key("profile/work/server/docs"));
        let rendered = format!(
            "{:?}",
            KeychainCredentialStore::new(
                store,
                &crate::runtime::ActiveProfile::Named("work".into()),
                "docs"
            )
        );
        assert!(!rendered.contains("access-sentinel"));
        assert!(!rendered.contains(&base));
    }

    #[derive(Default)]
    struct RecordingOpener(Mutex<Vec<String>>);

    impl BrowserOpener for RecordingOpener {
        fn open(&self, authorization_url: &str) -> anyhow::Result<()> {
            self.0.lock().unwrap().push(authorization_url.into());
            Ok(())
        }
    }

    async fn start_device_fixture() -> (
        String,
        tokio::task::JoinHandle<Vec<(String, String, Vec<u8>)>>,
    ) {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let fixture_base = base.clone();
        let handle = tokio::spawn(async move {
            let mut requests = Vec::new();
            let mut token_polls = 0;
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_request(&mut stream).await;
                let path = request.1.clone();
                if path.contains("oauth-protected-resource") {
                    write_json(
                        &mut stream,
                        "200 OK",
                        serde_json::json!({
                            "resource": format!("{fixture_base}/mcp"),
                            "authorization_servers": [fixture_base]
                        }),
                    )
                    .await;
                } else if path.contains("oauth-authorization-server")
                    || path.contains("openid-configuration")
                {
                    write_json(
                        &mut stream,
                        "200 OK",
                        serde_json::json!({
                            "issuer": fixture_base,
                            "authorization_endpoint": format!("{fixture_base}/authorize"),
                            "token_endpoint": format!("{fixture_base}/token"),
                            "device_authorization_endpoint": format!("{fixture_base}/device"),
                            "code_challenge_methods_supported": ["S256"]
                        }),
                    )
                    .await;
                } else if path == "/device" {
                    write_json(
                        &mut stream,
                        "200 OK",
                        serde_json::json!({
                            "device_code": "device-secret-sentinel",
                            "user_code": "ABCD-EFGH",
                            "verification_uri": format!("{fixture_base}/verify"),
                            "verification_uri_complete": format!("{fixture_base}/verify?user_code=ABCD-EFGH"),
                            "expires_in": 30,
                            "interval": 1
                        }),
                    )
                    .await;
                } else if path == "/token" {
                    token_polls += 1;
                    if token_polls == 1 {
                        write_json(
                            &mut stream,
                            "400 Bad Request",
                            serde_json::json!({"error":"authorization_pending"}),
                        )
                        .await;
                    } else {
                        write_json(
                            &mut stream,
                            "200 OK",
                            serde_json::json!({
                                "access_token": "device-access-sentinel",
                                "token_type": "Bearer",
                                "expires_in": 3600,
                                "refresh_token": "device-refresh-sentinel",
                                "scope": "tools"
                            }),
                        )
                        .await;
                    }
                } else {
                    write_json(&mut stream, "404 Not Found", serde_json::json!({})).await;
                }
                requests.push(request);
                if token_polls == 2 {
                    break;
                }
            }
            requests
        });
        (base, handle)
    }

    #[tokio::test]
    async fn advertised_device_flow_is_bounded_and_stores_rotatable_credentials() {
        let (base, fixture) = start_device_fixture().await;
        let store = Arc::new(FakeStore::default());
        let opener = Arc::new(RecordingOpener::default());
        let progress_values = Arc::new(Mutex::new(Vec::new()));
        let captured = progress_values.clone();
        let progress: ProgressSink = Arc::new(move |value| captured.lock().unwrap().push(value));
        let config = crate::config::McpServerConfig {
            transport: crate::config::McpTransport::StreamableHttp,
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            url: Some(format!("{base}/mcp")),
            headers: HashMap::new(),
            enabled: true,
            timeout_secs: 2,
            network: true,
            oauth: crate::config::McpOAuthConfig {
                enabled: true,
                flow: crate::config::McpOAuthFlow::Device,
                issuer: Some(base.clone()),
                client_id: Some("device-client".into()),
                scopes: vec!["tools".into()],
                ..Default::default()
            },
        };
        authorize_device(
            &config,
            &crate::runtime::ActiveProfile::Default,
            "docs",
            store.clone(),
            opener.clone(),
            Some(progress),
        )
        .await
        .unwrap();
        let requests = fixture.await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|(_, path, _)| path == "/device")
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|(_, path, _)| path == "/token")
                .count(),
            2
        );
        let device_body = requests
            .iter()
            .find(|(_, path, _)| path == "/device")
            .map(|(_, _, body)| String::from_utf8_lossy(body).into_owned())
            .unwrap();
        assert!(device_body.contains("resource="));
        assert_eq!(opener.0.lock().unwrap().len(), 1);
        let progress_values = progress_values.lock().unwrap();
        assert!(matches!(
            progress_values.first(),
            Some(McpOAuthProgress::DeviceCode { .. })
        ));
        assert!(matches!(
            progress_values.last(),
            Some(McpOAuthProgress::WaitingForAuthorization)
        ));
        assert!(!format!("{:?}", progress_values[0]).contains("ABCD-EFGH"));
        assert!(store.0.lock().unwrap().contains_key("server/docs"));
    }

    #[tokio::test]
    async fn expired_token_refresh_rotates_the_exact_stored_credential() {
        let (base, fixture) = start_oauth_fixture().await;
        let store = Arc::new(FakeStore::default());
        let expired: rmcp::transport::auth::OAuthTokenResponse =
            serde_json::from_value(serde_json::json!({
                "access_token":"old-access-sentinel",
                "token_type":"Bearer",
                "expires_in":1,
                "refresh_token":"old-refresh-sentinel",
                "scope":"tools"
            }))
            .unwrap();
        let stored = StoredCredentials::new(
            "fixture-client".into(),
            Some(expired),
            vec!["tools".into()],
            Some(1),
        )
        .with_issuer(Some(base.clone()));
        store.0.lock().unwrap().insert(
            "profile/work/server/docs".into(),
            serde_json::to_vec(&stored).unwrap(),
        );
        let config = crate::config::McpServerConfig {
            transport: crate::config::McpTransport::StreamableHttp,
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            url: Some(format!("{base}/mcp")),
            headers: HashMap::new(),
            enabled: true,
            timeout_secs: 2,
            network: true,
            oauth: crate::config::McpOAuthConfig {
                enabled: true,
                issuer: Some(base.clone()),
                client_id: Some("fixture-client".into()),
                scopes: vec!["tools".into()],
                ..Default::default()
            },
        };
        let (manager, initialized) = initialized_authorization_manager(
            &config,
            &crate::runtime::ActiveProfile::Named("work".into()),
            "docs",
            store.clone(),
        )
        .await
        .unwrap();
        assert!(initialized);
        assert_eq!(manager.get_access_token().await.unwrap(), "access-sentinel");
        let requests = fixture.await.unwrap();
        let refresh_body = requests
            .iter()
            .find(|(_, path, _)| path == "/token")
            .map(|(_, _, body)| String::from_utf8_lossy(body).into_owned())
            .unwrap();
        assert!(refresh_body.contains("grant_type=refresh_token"));
        assert!(refresh_body.contains("old-refresh-sentinel"));
        let saved = store
            .0
            .lock()
            .unwrap()
            .get("profile/work/server/docs")
            .cloned()
            .unwrap();
        let saved_text = String::from_utf8(saved).unwrap();
        assert!(saved_text.contains("access-sentinel"));
        assert!(!saved_text.contains("old-access-sentinel"));
    }

    #[tokio::test]
    async fn discovered_cross_origin_issuer_must_match_the_explicit_pin() {
        let (base, fixture) = start_oauth_fixture().await;
        let other = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let pinned = format!("http://{}", other.local_addr().unwrap());
        let config = crate::config::McpServerConfig {
            transport: crate::config::McpTransport::StreamableHttp,
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            url: Some(format!("{base}/mcp")),
            headers: HashMap::new(),
            enabled: true,
            timeout_secs: 1,
            network: true,
            oauth: crate::config::McpOAuthConfig {
                enabled: true,
                issuer: Some(pinned.clone()),
                client_id: Some("fixture-client".into()),
                ..Default::default()
            },
        };
        let error = match initialized_authorization_manager(
            &config,
            &crate::runtime::ActiveProfile::Default,
            "docs",
            Arc::new(FakeStore::default()),
        )
        .await
        {
            Ok(_) => panic!("issuer mismatch unexpectedly succeeded"),
            Err(error) => error,
        };
        let safe = error.to_string();
        assert!(!safe.contains(&base));
        assert!(!safe.contains(&pinned));
        drop(other);
        let _ = fixture.await.unwrap();
    }

    #[tokio::test]
    async fn browser_authorization_abort_leaves_no_partial_credential() {
        let (base, fixture) = start_oauth_fixture().await;
        let store = Arc::new(FakeStore::default());
        let config = crate::config::McpServerConfig {
            transport: crate::config::McpTransport::StreamableHttp,
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            url: Some(format!("{base}/mcp")),
            headers: HashMap::new(),
            enabled: true,
            timeout_secs: 1,
            network: true,
            oauth: crate::config::McpOAuthConfig {
                enabled: true,
                issuer: Some(base),
                client_id: Some("fixture-client".into()),
                ..Default::default()
            },
        };
        let task_store = store.clone();
        let task = tokio::spawn(async move {
            authorize_browser(
                &config,
                &crate::runtime::ActiveProfile::Default,
                "docs",
                task_store,
                Arc::new(NoCallbackOpener),
                None,
            )
            .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(store.0.lock().unwrap().is_empty());
        let _ = fixture.await.unwrap();
    }

    #[tokio::test]
    async fn callback_requires_exact_loopback_host_and_path() {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let redirect = format!("http://{address}/oauth/callback");
        let task = tokio::spawn(async move { receive_callback(listener, &redirect).await });
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream
            .write_all(
                b"GET /oauth/callback?code=x&state=y HTTP/1.1\r\nHost: attacker.invalid\r\n\r\n",
            )
            .await
            .unwrap();
        assert_eq!(task.await.unwrap(), Err(McpOAuthActionError::Callback));
    }

    #[test]
    fn device_poll_outcomes_are_typed_and_body_free() {
        assert_eq!(
            device_poll_action(Some("authorization_pending")),
            DevicePollAction::Continue
        );
        assert_eq!(
            device_poll_action(Some("slow_down")),
            DevicePollAction::SlowDown
        );
        assert_eq!(
            device_poll_action(Some("expired_token")),
            DevicePollAction::Expired
        );
        assert_eq!(
            device_poll_action(Some("access_denied")),
            DevicePollAction::Rejected
        );
        assert_eq!(
            device_poll_action(Some("remote-body-sentinel")),
            DevicePollAction::Rejected
        );
    }

    #[tokio::test]
    async fn missing_client_secret_environment_fails_before_network_io() {
        const ENVIRONMENT: &str = "CA_MCP063_MISSING_CLIENT_SECRET";
        std::env::remove_var(ENVIRONMENT);
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let config = crate::config::McpServerConfig {
            transport: crate::config::McpTransport::StreamableHttp,
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            url: Some(format!("{base}/mcp")),
            headers: HashMap::new(),
            enabled: true,
            timeout_secs: 1,
            network: true,
            oauth: crate::config::McpOAuthConfig {
                enabled: true,
                issuer: Some(base.clone()),
                client_id: Some("fixture-client".into()),
                client_secret: Some(format!("${{{ENVIRONMENT}}}")),
                ..Default::default()
            },
        };
        assert!(initialized_authorization_manager(
            &config,
            &crate::runtime::ActiveProfile::Default,
            "docs",
            Arc::new(FakeStore::default()),
        )
        .await
        .is_err());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    }
}
