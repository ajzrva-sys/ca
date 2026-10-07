use anyhow::Result;
use base64::Engine;
use futures::StreamExt as _;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use ring::digest;
use ring::rand::SecureRandom;
use serde::{Deserialize, Serialize};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const ISSUER: &str = "https://auth.openai.com";
const AUTHORIZE_PATH: &str = "/oauth/authorize";
const TOKEN_PATH: &str = "/oauth/token";
const REVOKE_PATH: &str = "/oauth/revoke";
const REDIRECT_PORT_PRIMARY: u16 = 1455;
const REDIRECT_PORT_FALLBACK: u16 = 1457;
const SCOPES: &str =
    "openid profile email offline_access api.connectors.read api.connectors.invoke";
#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "ca.codex-oauth";
#[cfg(target_os = "macos")]
const KEYCHAIN_ACCOUNT: &str = "chatgpt-tokens";
const CALLBACK_TIMEOUT_SECS: u64 = 300;
#[cfg(any(not(target_os = "macos"), test))]
const DEVICE_TIMEOUT_SECS: u64 = 15 * 60;
#[cfg(any(not(target_os = "macos"), test))]
const DEVICE_VERIFICATION_URL: &str = "https://auth.openai.com/codex/device";
const MAX_AUTH_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_AUTH_CACHE_BYTES: u64 = 1024 * 1024;

const ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

#[derive(Clone, Serialize, Deserialize)]
pub struct CodexTokens {
    pub access_token: String,
    pub id_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub account_id: Option<String>,
}

impl std::fmt::Debug for CodexTokens {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CodexTokens")
            .field("access_token", &"[REDACTED]")
            .field("id_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field(
                "account_id",
                &self.account_id.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum CodexAuthStatus {
    Connected,
    Disconnected,
}

#[derive(Debug, thiserror::Error)]
pub enum CodexAuthError {
    #[error("OAuth browser flow failed: {0}")]
    Browser(String),
    #[error("OAuth token exchange failed: {0}")]
    TokenExchange(String),
    #[error("OAuth token refresh failed: {0}")]
    Refresh(String),
    #[error("OAuth callback timed out")]
    TimedOut,
    #[error("OAuth callback was invalid: {0}")]
    Callback(String),
    #[error("OAuth state mismatch")]
    StateMismatch,
    #[error("Device authorization failed: {0}")]
    Device(String),
    #[error("Device authorization was denied")]
    #[cfg(any(not(target_os = "macos"), test))]
    DeviceDenied,
    #[error("Device authorization expired")]
    #[cfg(any(not(target_os = "macos"), test))]
    DeviceExpired,
    #[error("OAuth revoked by user")]
    #[allow(dead_code)]
    Revoked,
    #[error("Keychain error: {0}")]
    #[cfg(target_os = "macos")]
    Keychain(String),
    #[error("Codex credential cache error: {0}")]
    CredentialCache(String),
    #[error("Not connected")]
    NotConnected,
}

#[cfg(any(not(target_os = "macos"), test))]
#[derive(Clone, PartialEq, Eq)]
pub struct DeviceAuthorization {
    pub verification_url: String,
    pub user_code: String,
    device_auth_id: String,
    interval_secs: u64,
}

#[cfg(any(not(target_os = "macos"), test))]
impl std::fmt::Debug for DeviceAuthorization {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeviceAuthorization")
            .field("verification_url", &self.verification_url)
            .field("user_code", &"[REDACTED]")
            .field("device_auth_id", &"[REDACTED]")
            .field("interval_secs", &self.interval_secs)
            .finish()
    }
}

fn auth_cache_path() -> Result<PathBuf, CodexAuthError> {
    if let Some(home) = std::env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home).join("auth.json"));
    }
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CodexAuthError::CredentialCache("HOME is not set".into()))?;
    Ok(PathBuf::from(home).join(".codex/auth.json"))
}

fn validate_cache_metadata(
    path: &Path,
    metadata: &std::fs::Metadata,
) -> Result<(), CodexAuthError> {
    if !metadata.file_type().is_file() {
        return Err(CodexAuthError::CredentialCache(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(CodexAuthError::CredentialCache(format!(
                "{} is not owned by the current user",
                path.display()
            )));
        }
        if metadata.mode() & 0o077 != 0 {
            return Err(CodexAuthError::CredentialCache(format!(
                "{} must have mode 0600 or stricter",
                path.display()
            )));
        }
    }
    Ok(())
}

fn read_auth_cache_value(path: &Path) -> Result<Option<serde_json::Value>, CodexAuthError> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(CodexAuthError::CredentialCache(format!(
                "could not open {}: {error}",
                path.display()
            )))
        }
    };
    let metadata = file.metadata().map_err(|error| {
        CodexAuthError::CredentialCache(format!("could not inspect {}: {error}", path.display()))
    })?;
    validate_cache_metadata(path, &metadata)?;
    if metadata.len() > MAX_AUTH_CACHE_BYTES {
        return Err(CodexAuthError::CredentialCache(format!(
            "{} is too large",
            path.display()
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_AUTH_CACHE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            CodexAuthError::CredentialCache(format!("could not read {}: {error}", path.display()))
        })?;
    if bytes.len() as u64 > MAX_AUTH_CACHE_BYTES {
        bytes.fill(0);
        return Err(CodexAuthError::CredentialCache(format!(
            "{} is too large",
            path.display()
        )));
    }
    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        CodexAuthError::CredentialCache(format!("{} is not valid JSON: {error}", path.display()))
    })
}

fn tokens_from_cache_value(value: &serde_json::Value) -> Result<CodexTokens, CodexAuthError> {
    let tokens = value.get("tokens").ok_or_else(|| {
        CodexAuthError::CredentialCache("auth.json does not contain tokens".into())
    })?;
    serde_json::from_value(tokens.clone()).map_err(|error| {
        CodexAuthError::CredentialCache(format!("auth.json has invalid token fields: {error}"))
    })
}

fn load_file_tokens_at(path: &Path) -> Result<Option<CodexTokens>, CodexAuthError> {
    read_auth_cache_value(path)?
        .map(|value| tokens_from_cache_value(&value))
        .transpose()
}

fn write_file_tokens_at(path: &Path, tokens: &CodexTokens) -> Result<(), CodexAuthError> {
    let mut value = read_auth_cache_value(path)?.unwrap_or_else(|| {
        serde_json::json!({
            "auth_mode": "chatgpt"
        })
    });
    let object = value.as_object_mut().ok_or_else(|| {
        CodexAuthError::CredentialCache("auth.json root must be an object".into())
    })?;
    object.insert(
        "tokens".into(),
        serde_json::to_value(tokens)
            .map_err(|error| CodexAuthError::CredentialCache(error.to_string()))?,
    );
    object.insert(
        "last_refresh".into(),
        serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
    );
    let bytes = serde_json::to_vec_pretty(&value)
        .map_err(|error| CodexAuthError::CredentialCache(error.to_string()))?;
    let parent = path
        .parent()
        .ok_or_else(|| CodexAuthError::CredentialCache("auth.json path has no parent".into()))?;
    ensure_private_cache_directory(parent)?;
    let temporary = parent.join(format!(
        ".auth.json.ca-{}-{}.tmp",
        std::process::id(),
        base64url_encode(&generate_random_bytes(12))
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let result = (|| {
        let mut file = options.open(&temporary).map_err(|error| {
            CodexAuthError::CredentialCache(format!("could not create refresh file: {error}"))
        })?;
        file.write_all(&bytes).map_err(|error| {
            CodexAuthError::CredentialCache(format!("could not write refresh file: {error}"))
        })?;
        file.sync_all().map_err(|error| {
            CodexAuthError::CredentialCache(format!("could not sync refresh file: {error}"))
        })?;
        std::fs::rename(&temporary, path).map_err(|error| {
            CodexAuthError::CredentialCache(format!("could not replace auth.json: {error}"))
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn ensure_private_cache_directory(path: &Path) -> Result<(), CodexAuthError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};
        if !path.exists() {
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true).mode(0o700);
            builder.create(path).map_err(|error| {
                CodexAuthError::CredentialCache(format!(
                    "could not create {}: {error}",
                    path.display()
                ))
            })?;
        }
        let metadata = std::fs::symlink_metadata(path).map_err(|error| {
            CodexAuthError::CredentialCache(format!(
                "could not inspect {}: {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(CodexAuthError::CredentialCache(format!(
                "{} is not a current-user directory",
                path.display()
            )));
        }
        if metadata.mode() & 0o077 != 0 {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(
                |error| {
                    CodexAuthError::CredentialCache(format!(
                        "could not secure {}: {error}",
                        path.display()
                    ))
                },
            )?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(CodexAuthError::CredentialCache(
            "file credential caching requires a Unix host".into(),
        ))
    }
}

fn generate_random_bytes(count: usize) -> Vec<u8> {
    let rng = ring::rand::SystemRandom::new();
    let mut bytes = vec![0u8; count];
    rng.fill(&mut bytes).expect("system RNG is available");
    bytes
}

fn base64url_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn generate_pkce() -> (String, String) {
    let verifier_bytes = generate_random_bytes(64);
    let code_verifier = base64url_encode(&verifier_bytes);
    let digest = digest::digest(&digest::SHA256, code_verifier.as_bytes());
    let code_challenge = base64url_encode(digest.as_ref());
    (code_verifier, code_challenge)
}

fn pct_encode(value: &str) -> String {
    utf8_percent_encode(value, ENCODE_SET).to_string()
}

fn build_authorization_url(code_challenge: &str, state: &str, port: u16) -> String {
    let redirect_uri = format!("http://localhost:{port}/auth/callback");
    format!(
        "{ISSUER}{AUTHORIZE_PATH}?\
         response_type=code\
         &client_id={CLIENT_ID}\
         &redirect_uri={}\
         &scope={}\
         &code_challenge={code_challenge}\
         &code_challenge_method=S256\
         &state={state}\
         &id_token_add_organizations=true\
         &codex_cli_simplified_flow=true\
         &originator=codex_cli_rs",
        pct_encode(&redirect_uri),
        pct_encode(SCOPES),
    )
}

fn redirect_uri(port: u16) -> String {
    format!("http://localhost:{port}/auth/callback")
}

async fn receive_callback(
    listener: tokio::net::TcpListener,
    expected_host: String,
) -> Result<(String, String), CodexAuthError> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let (mut stream, peer) = listener
        .accept()
        .await
        .map_err(|e| CodexAuthError::Callback(e.to_string()))?;
    if !peer.ip().is_loopback() {
        return Err(CodexAuthError::Callback("non-loopback peer".into()));
    }
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 2048];
        let count = stream
            .read(&mut chunk)
            .await
            .map_err(|e| CodexAuthError::Callback(e.to_string()))?;
        if count == 0 || bytes.len().saturating_add(count) > 16 * 1024 {
            return Err(CodexAuthError::Callback("oversized request".into()));
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| CodexAuthError::Callback("invalid UTF-8".into()))?;
    let mut lines = headers.split("\r\n");
    let request_line = lines
        .next()
        .ok_or(CodexAuthError::Callback("empty request".into()))?;
    let mut parts = request_line.split_whitespace();
    if parts.next() != Some("GET") {
        return Err(CodexAuthError::Callback("not GET".into()));
    }
    let target = parts
        .next()
        .ok_or(CodexAuthError::Callback("no target".into()))?;
    if parts.next() != Some("HTTP/1.1") || !target.starts_with("/auth/callback?") {
        return Err(CodexAuthError::Callback("unexpected path".into()));
    }
    let host = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.trim());
    if host != Some(expected_host.as_str()) {
        return Err(CodexAuthError::Callback("host mismatch".into()));
    }
    let query = target.strip_prefix("/auth/callback?").unwrap_or("");
    let params: std::collections::HashMap<String, String> =
        url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect();
    let code = params
        .get("code")
        .cloned()
        .ok_or_else(|| CodexAuthError::Callback("missing code".into()))?;
    let state = params
        .get("state")
        .cloned()
        .ok_or_else(|| CodexAuthError::Callback("missing state".into()))?;
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n\
              <html><body><h1>Authorization complete</h1><p>You can close this tab and return to ca.</p></body></html>",
        )
        .await
        .map_err(|e| CodexAuthError::Callback(e.to_string()))?;
    Ok((code, state))
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    id_token: Option<String>,
    refresh_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

impl TokenResponse {
    fn into_tokens(self) -> Result<CodexTokens, CodexAuthError> {
        if let Some(error) = self.error {
            let _ = (error, self.error_description);
            return Err(CodexAuthError::TokenExchange(
                "authorization server rejected the exchange".into(),
            ));
        }
        Ok(CodexTokens {
            access_token: self
                .access_token
                .ok_or_else(|| CodexAuthError::TokenExchange("missing access_token".into()))?,
            id_token: self
                .id_token
                .ok_or_else(|| CodexAuthError::TokenExchange("missing id_token".into()))?,
            refresh_token: self
                .refresh_token
                .ok_or_else(|| CodexAuthError::TokenExchange("missing refresh_token".into()))?,
            account_id: None,
        })
    }
}

async fn exchange_code_for_tokens(
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<CodexTokens, CodexAuthError> {
    let client = auth_http_client()?;
    exchange_code_for_tokens_at(&client, ISSUER, code, code_verifier, redirect_uri).await
}

async fn exchange_code_for_tokens_at(
    client: &reqwest::Client,
    issuer: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<CodexTokens, CodexAuthError> {
    let response = client
        .post(format!("{issuer}{TOKEN_PATH}"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", CLIENT_ID),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await
        .map_err(|e| CodexAuthError::TokenExchange(e.to_string()))?;
    if !response.status().is_success() {
        return Err(CodexAuthError::TokenExchange(format!(
            "authorization server returned HTTP {}",
            response.status().as_u16()
        )));
    }
    let token_resp: TokenResponse = bounded_json(response, "token exchange")
        .await
        .map_err(|error| CodexAuthError::TokenExchange(error.to_string()))?;
    let mut tokens = token_resp.into_tokens()?;
    tokens.account_id = extract_account_id(&tokens.id_token);
    Ok(tokens)
}

fn auth_http_client() -> Result<reqwest::Client, CodexAuthError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| CodexAuthError::Device(error.to_string()))
}

async fn bounded_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    operation: &str,
) -> Result<T, CodexAuthError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_AUTH_RESPONSE_BYTES as u64)
    {
        return Err(CodexAuthError::Device(format!(
            "{operation} response was too large"
        )));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| {
            CodexAuthError::Device(format!("{operation} response could not be read"))
        })?;
        if bytes.len().saturating_add(chunk.len()) > MAX_AUTH_RESPONSE_BYTES {
            return Err(CodexAuthError::Device(format!(
                "{operation} response was too large"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| CodexAuthError::Device(format!("{operation} response was invalid")))
}

#[cfg(any(not(target_os = "macos"), test))]
#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_auth_id: String,
    #[serde(alias = "usercode", alias = "userCode")]
    user_code: String,
    #[serde(default)]
    interval: serde_json::Value,
}

#[cfg(any(not(target_os = "macos"), test))]
#[derive(Deserialize)]
struct DeviceTokenResponse {
    authorization_code: String,
    code_challenge: String,
    code_verifier: String,
}

#[cfg(any(not(target_os = "macos"), test))]
fn device_interval(value: &serde_json::Value) -> u64 {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        .unwrap_or(5)
        .clamp(1, 15)
}

#[cfg(any(not(target_os = "macos"), test))]
pub async fn request_device_authorization() -> Result<DeviceAuthorization, CodexAuthError> {
    let client = auth_http_client()?;
    request_device_authorization_at(&client, ISSUER).await
}

#[cfg(any(not(target_os = "macos"), test))]
async fn request_device_authorization_at(
    client: &reqwest::Client,
    issuer: &str,
) -> Result<DeviceAuthorization, CodexAuthError> {
    let response = client
        .post(format!("{issuer}/api/accounts/deviceauth/usercode"))
        .json(&serde_json::json!({ "client_id": CLIENT_ID }))
        .send()
        .await
        .map_err(|_| CodexAuthError::Device("could not request a device code".into()))?;
    if !response.status().is_success() {
        return Err(CodexAuthError::Device(format!(
            "device-code server returned HTTP {}",
            response.status().as_u16()
        )));
    }
    let response: DeviceCodeResponse = bounded_json(response, "device-code").await?;
    if response.device_auth_id.is_empty()
        || response.user_code.is_empty()
        || response.device_auth_id.len() > 4096
        || response.user_code.len() > 256
    {
        return Err(CodexAuthError::Device(
            "device-code response was invalid".into(),
        ));
    }
    Ok(DeviceAuthorization {
        verification_url: DEVICE_VERIFICATION_URL.into(),
        user_code: response.user_code,
        device_auth_id: response.device_auth_id,
        interval_secs: device_interval(&response.interval),
    })
}

#[cfg(any(not(target_os = "macos"), test))]
pub async fn complete_device_authorization_candidate(
    authorization: &DeviceAuthorization,
) -> Result<CodexTokens, CodexAuthError> {
    let client = auth_http_client()?;
    complete_device_authorization_at(
        &client,
        ISSUER,
        authorization,
        std::time::Duration::from_secs(DEVICE_TIMEOUT_SECS),
    )
    .await
}

#[cfg(any(not(target_os = "macos"), test))]
async fn complete_device_authorization_at(
    client: &reqwest::Client,
    issuer: &str,
    authorization: &DeviceAuthorization,
    timeout: std::time::Duration,
) -> Result<CodexTokens, CodexAuthError> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(CodexAuthError::DeviceExpired);
        }
        let response = client
            .post(format!("{issuer}/api/accounts/deviceauth/token"))
            .json(&serde_json::json!({
                "device_auth_id": authorization.device_auth_id,
                "user_code": authorization.user_code,
            }))
            .send()
            .await
            .map_err(|_| CodexAuthError::Device("device authorization polling failed".into()))?;
        match response.status().as_u16() {
            200 => {
                let response: DeviceTokenResponse =
                    bounded_json(response, "device authorization").await?;
                if response.authorization_code.is_empty()
                    || response.code_verifier.is_empty()
                    || response.code_challenge.is_empty()
                {
                    return Err(CodexAuthError::Device(
                        "device authorization response was invalid".into(),
                    ));
                }
                let digest = digest::digest(&digest::SHA256, response.code_verifier.as_bytes());
                if base64url_encode(digest.as_ref()) != response.code_challenge {
                    return Err(CodexAuthError::Device(
                        "device authorization proof was invalid".into(),
                    ));
                }
                return exchange_code_for_tokens_at(
                    client,
                    issuer,
                    &response.authorization_code,
                    &response.code_verifier,
                    "https://auth.openai.com/deviceauth/callback",
                )
                .await;
            }
            403 | 404 => {
                tokio::time::sleep(std::time::Duration::from_secs(authorization.interval_secs))
                    .await;
            }
            400 | 401 => return Err(CodexAuthError::DeviceDenied),
            status => {
                return Err(CodexAuthError::Device(format!(
                    "device authorization server returned HTTP {status}"
                )))
            }
        }
    }
}

fn extract_account_id(id_token: &str) -> Option<String> {
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    claims
        .get("chatgpt_account_id")
        .or_else(|| {
            claims
                .get("https://api.openai.com/auth")?
                .get("chatgpt_account_id")
        })
        .or_else(|| claims.get("sub"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

pub async fn authorize_browser_candidate() -> Result<CodexTokens, CodexAuthError> {
    authorize_browser_candidate_with_progress(|_| {}).await
}

pub async fn authorize_browser_candidate_with_progress(
    progress: impl FnOnce(String) + Send,
) -> Result<CodexTokens, CodexAuthError> {
    let (code_verifier, code_challenge) = generate_pkce();
    let state_bytes = generate_random_bytes(32);
    let state = base64url_encode(&state_bytes);

    let (listener, port) =
        match tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, REDIRECT_PORT_PRIMARY))
            .await
        {
            Ok(l) => (l, REDIRECT_PORT_PRIMARY),
            Err(_) => tokio::net::TcpListener::bind((
                std::net::Ipv4Addr::LOCALHOST,
                REDIRECT_PORT_FALLBACK,
            ))
            .await
            .map(|l| (l, REDIRECT_PORT_FALLBACK))
            .map_err(|e| CodexAuthError::Browser(format!("bind failed: {e}")))?,
        };

    let redirect = redirect_uri(port);
    let expected_host = format!("localhost:{port}");
    let auth_url = build_authorization_url(&code_challenge, &state, port);
    progress(auth_url.clone());

    tokio::task::spawn_blocking(move || open_browser(&auth_url))
        .await
        .map_err(|e| CodexAuthError::Browser(e.to_string()))?
        .map_err(|e: CodexAuthError| e)?;

    let (code, returned_state) = tokio::time::timeout(
        std::time::Duration::from_secs(CALLBACK_TIMEOUT_SECS),
        receive_callback(listener, expected_host),
    )
    .await
    .map_err(|_| CodexAuthError::TimedOut)??;

    if returned_state != state {
        return Err(CodexAuthError::StateMismatch);
    }

    let mut tokens = exchange_code_for_tokens(&code, &code_verifier, &redirect).await?;
    tokens.account_id = extract_account_id(&tokens.id_token);
    Ok(tokens)
}

pub async fn authorize_browser() -> Result<CodexTokens, CodexAuthError> {
    let tokens = authorize_browser_candidate().await?;
    store_tokens(&tokens)?;
    Ok(tokens)
}

fn open_browser(url: &str) -> Result<(), CodexAuthError> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/open")
            .arg(url)
            .status()
            .map_err(|e| CodexAuthError::Browser(e.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            Err(CodexAuthError::Browser(format!(
                "open exited with {status}"
            )))
        }
    }
    #[cfg(target_os = "linux")]
    {
        let status = std::process::Command::new("xdg-open")
            .arg(url)
            .status()
            .map_err(|e| CodexAuthError::Browser(e.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            Err(CodexAuthError::Browser(format!(
                "xdg-open exited with {status}"
            )))
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = url;
        Err(CodexAuthError::Browser(
            "automatic browser opening is not supported on this platform; open the URL manually"
                .into(),
        ))
    }
}

pub async fn refresh_tokens(tokens: &CodexTokens) -> Result<CodexTokens, CodexAuthError> {
    let client = auth_http_client().map_err(|error| CodexAuthError::Refresh(error.to_string()))?;
    let response = client
        .post(format!("{ISSUER}{TOKEN_PATH}"))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &tokens.refresh_token),
            ("client_id", CLIENT_ID),
        ])
        .send()
        .await
        .map_err(|e| CodexAuthError::Refresh(e.to_string()))?;
    if !response.status().is_success() {
        return Err(CodexAuthError::Refresh(format!(
            "authorization server returned HTTP {}",
            response.status().as_u16()
        )));
    }
    let token_resp: TokenResponse = bounded_json(response, "token refresh")
        .await
        .map_err(|error| CodexAuthError::Refresh(error.to_string()))?;
    let mut new_tokens = token_resp.into_tokens().map_err(|e| match e {
        CodexAuthError::TokenExchange(msg) => CodexAuthError::Refresh(msg),
        other => other,
    })?;
    new_tokens.account_id = extract_account_id(&new_tokens.id_token);
    store_tokens(&new_tokens)?;
    Ok(new_tokens)
}

pub async fn revoke_tokens() -> Result<(), CodexAuthError> {
    let tokens = load_tokens()?.ok_or(CodexAuthError::NotConnected)?;
    let client = reqwest::Client::new();
    let _ = client
        .post(format!("{ISSUER}{REVOKE_PATH}"))
        .form(&[
            ("token", tokens.refresh_token.as_str()),
            ("client_id", CLIENT_ID),
        ])
        .send()
        .await;
    delete_tokens()
}

pub fn store_tokens(tokens: &CodexTokens) -> Result<(), CodexAuthError> {
    let cache_path = auth_cache_path()?;
    if cache_path.exists() {
        return write_file_tokens_at(&cache_path, tokens);
    }
    #[cfg(target_os = "macos")]
    {
        let json =
            serde_json::to_vec(tokens).map_err(|e| CodexAuthError::Keychain(e.to_string()))?;
        security_framework::passwords::set_generic_password(
            KEYCHAIN_SERVICE,
            KEYCHAIN_ACCOUNT,
            &json,
        )
        .map_err(|e| CodexAuthError::Keychain(e.to_string()))?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        write_file_tokens_at(&cache_path, tokens)
    }
}

pub fn load_tokens() -> Result<Option<CodexTokens>, CodexAuthError> {
    if let Some(tokens) = load_file_tokens_at(&auth_cache_path()?)? {
        return Ok(Some(tokens));
    }
    #[cfg(target_os = "macos")]
    {
        let options = security_framework::passwords::PasswordOptions::new_generic_password(
            KEYCHAIN_SERVICE,
            KEYCHAIN_ACCOUNT,
        );
        match security_framework::passwords::generic_password(options) {
            Ok(bytes) => {
                let tokens: CodexTokens = serde_json::from_slice(&bytes)
                    .map_err(|e| CodexAuthError::Keychain(e.to_string()))?;
                Ok(Some(tokens))
            }
            Err(error) if error.code() == -25_300 => Ok(None),
            Err(error) => Err(CodexAuthError::Keychain(error.to_string())),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(None)
    }
}

pub fn delete_tokens() -> Result<(), CodexAuthError> {
    let cache_path = auth_cache_path()?;
    if read_auth_cache_value(&cache_path)?.is_some() {
        return std::fs::remove_file(&cache_path).map_err(|error| {
            CodexAuthError::CredentialCache(format!(
                "could not remove {}: {error}",
                cache_path.display()
            ))
        });
    }
    #[cfg(target_os = "macos")]
    {
        match security_framework::passwords::delete_generic_password(
            KEYCHAIN_SERVICE,
            KEYCHAIN_ACCOUNT,
        ) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == -25_300 => Ok(()),
            Err(error) => Err(CodexAuthError::Keychain(error.to_string())),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

pub fn tokens_valid(tokens: &CodexTokens) -> bool {
    let parts: Vec<&str> = tokens.access_token.split('.').collect();
    if parts.len() < 2 {
        return false;
    }
    let Ok(payload) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[1]) else {
        return false;
    };
    let Ok(claims) = serde_json::from_slice::<serde_json::Value>(&payload) else {
        return false;
    };
    let Some(exp) = claims.get("exp").and_then(|v| v.as_u64()) else {
        return false;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    exp > now + 60
}

#[allow(dead_code)]
pub fn auth_status() -> CodexAuthStatus {
    match load_tokens() {
        Ok(Some(_)) => CodexAuthStatus::Connected,
        _ => CodexAuthStatus::Disconnected,
    }
}

pub async fn resolve_credential() -> Result<String, CodexAuthError> {
    let mut tokens = load_tokens()?.ok_or(CodexAuthError::NotConnected)?;
    if !tokens_valid(&tokens) {
        tokens = refresh_tokens(&tokens).await?;
    }
    Ok(tokens.access_token.clone())
}

pub fn account_id_for_access_token(access_token: &str) -> Option<String> {
    account_id_for_access_token_at(&auth_cache_path().ok()?, access_token)
}

fn account_id_for_access_token_at(path: &Path, access_token: &str) -> Option<String> {
    let tokens = load_file_tokens_at(path).ok()??;
    (tokens.access_token == access_token)
        .then_some(tokens.account_id)
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    fn cache_fixture(path: &Path) {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "auth_mode": "chatgpt",
                "last_refresh": "2026-01-01T00:00:00Z",
                "unrelated": {"preserve": true},
                "tokens": {
                    "access_token": "access-one",
                    "id_token": "id-one",
                    "refresh_token": "refresh-one",
                    "account_id": "account-one"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
    }

    #[test]
    fn owner_only_codex_cache_loads_without_exposing_values() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("auth.json");
        cache_fixture(&path);

        let tokens = load_file_tokens_at(&path).unwrap().unwrap();

        assert_eq!(tokens.access_token, "access-one");
        assert_eq!(tokens.account_id.as_deref(), Some("account-one"));
        assert_eq!(
            account_id_for_access_token_at(&path, "access-one").as_deref(),
            Some("account-one")
        );
        assert_eq!(account_id_for_access_token_at(&path, "other"), None);
    }

    #[cfg(unix)]
    #[test]
    fn codex_cache_rejects_group_or_world_access() {
        use std::os::unix::fs::PermissionsExt as _;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("auth.json");
        cache_fixture(&path);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

        let error = load_file_tokens_at(&path).unwrap_err().to_string();

        assert!(error.contains("mode 0600 or stricter"));
        assert!(!error.contains("access-one"));
    }

    #[cfg(unix)]
    #[test]
    fn codex_cache_rejects_symlinks() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real.json");
        let link = temp.path().join("auth.json");
        cache_fixture(&real);
        symlink(&real, &link).unwrap();

        assert!(load_file_tokens_at(&link).is_err());
    }

    #[test]
    fn refresh_write_preserves_other_cache_fields() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("auth.json");
        cache_fixture(&path);
        let replacement = CodexTokens {
            access_token: "access-two".into(),
            id_token: "id-two".into(),
            refresh_token: "refresh-two".into(),
            account_id: Some("account-two".into()),
        };

        write_file_tokens_at(&path, &replacement).unwrap();

        let value = read_auth_cache_value(&path).unwrap().unwrap();
        assert_eq!(value["auth_mode"], "chatgpt");
        assert_eq!(value["unrelated"]["preserve"], true);
        assert_ne!(value["last_refresh"], "2026-01-01T00:00:00Z");
        let loaded = load_file_tokens_at(&path).unwrap().unwrap();
        assert_eq!(loaded.access_token, replacement.access_token);
        assert_eq!(loaded.id_token, replacement.id_token);
        assert_eq!(loaded.refresh_token, replacement.refresh_token);
        assert_eq!(loaded.account_id, replacement.account_id);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn missing_codex_cache_is_created_private_and_valid() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".codex/auth.json");
        let tokens = CodexTokens {
            access_token: "access-new".into(),
            id_token: "id-new".into(),
            refresh_token: "refresh-new".into(),
            account_id: Some("account-new".into()),
        };

        write_file_tokens_at(&path, &tokens).unwrap();

        let value = read_auth_cache_value(&path).unwrap().unwrap();
        assert_eq!(value["auth_mode"], "chatgpt");
        assert_eq!(
            load_file_tokens_at(&path).unwrap().unwrap().access_token,
            "access-new"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    async fn auth_fixture(
        responses: Vec<(u16, serde_json::Value)>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let mut requests = Vec::new();
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let header_end = loop {
                    let mut chunk = [0_u8; 2048];
                    let count = stream.read(&mut chunk).await.unwrap();
                    bytes.extend_from_slice(&chunk[..count]);
                    if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                        break index + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let content_length = headers
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                while bytes.len() < header_end + content_length {
                    let mut chunk = [0_u8; 2048];
                    let count = stream.read(&mut chunk).await.unwrap();
                    bytes.extend_from_slice(&chunk[..count]);
                }
                requests.push(String::from_utf8_lossy(&bytes).into_owned());
                let body = serde_json::to_vec(&body).unwrap();
                let reason = if status == 200 { "OK" } else { "Pending" };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.write_all(&body).await.unwrap();
            }
            requests
        });
        (format!("http://{address}"), handle)
    }

    #[tokio::test]
    async fn device_flow_polls_proves_pkce_and_exchanges_tokens() {
        let verifier = "device-fixture-verifier";
        let challenge =
            base64url_encode(digest::digest(&digest::SHA256, verifier.as_bytes()).as_ref());
        let payload =
            base64url_encode(br#"{"chatgpt_account_id":"fixture-account","exp":9999999999}"#);
        let (issuer, requests) = auth_fixture(vec![
            (
                200,
                serde_json::json!({
                    "device_auth_id": "device-id",
                    "user_code": "ABCD-EFGH",
                    "interval": "5"
                }),
            ),
            (403, serde_json::json!({"pending": true})),
            (
                200,
                serde_json::json!({
                    "authorization_code": "authorization-code",
                    "code_challenge": challenge,
                    "code_verifier": verifier
                }),
            ),
            (
                200,
                serde_json::json!({
                    "access_token": format!("header.{payload}.sig"),
                    "id_token": format!("header.{payload}.sig"),
                    "refresh_token": "refresh-token"
                }),
            ),
        ])
        .await;
        let client = auth_http_client().unwrap();
        let mut authorization = request_device_authorization_at(&client, &issuer)
            .await
            .unwrap();
        assert_eq!(authorization.verification_url, DEVICE_VERIFICATION_URL);
        assert_eq!(authorization.user_code, "ABCD-EFGH");
        authorization.interval_secs = 0;
        let tokens = complete_device_authorization_at(
            &client,
            &issuer,
            &authorization,
            std::time::Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(tokens.account_id.as_deref(), Some("fixture-account"));
        let requests = requests.await.unwrap();
        assert!(requests[0].starts_with("POST /api/accounts/deviceauth/usercode"));
        assert!(requests[1].starts_with("POST /api/accounts/deviceauth/token"));
        assert!(requests[3].starts_with("POST /oauth/token"));
        assert!(requests[3].contains("code_verifier=device-fixture-verifier"));
        let _public_request = request_device_authorization;
        let _public_complete = complete_device_authorization_candidate;
        assert_eq!(DEVICE_TIMEOUT_SECS, 900);
    }

    #[tokio::test]
    async fn device_flow_handles_denial_expiry_and_oversized_response() {
        let client = auth_http_client().unwrap();
        let authorization = DeviceAuthorization {
            verification_url: DEVICE_VERIFICATION_URL.into(),
            user_code: "code".into(),
            device_auth_id: "device".into(),
            interval_secs: 0,
        };
        let expired = complete_device_authorization_at(
            &client,
            "http://127.0.0.1:1",
            &authorization,
            std::time::Duration::ZERO,
        )
        .await;
        assert!(matches!(expired, Err(CodexAuthError::DeviceExpired)));

        let (issuer, server) = auth_fixture(vec![(400, serde_json::json!({"denied": true}))]).await;
        let denied = complete_device_authorization_at(
            &client,
            &issuer,
            &authorization,
            std::time::Duration::from_secs(1),
        )
        .await;
        assert!(matches!(denied, Err(CodexAuthError::DeviceDenied)));
        server.await.unwrap();

        let oversized = serde_json::json!({
            "device_auth_id": "x".repeat(MAX_AUTH_RESPONSE_BYTES),
            "user_code": "code"
        });
        let (issuer, server) = auth_fixture(vec![(200, oversized)]).await;
        let error = request_device_authorization_at(&client, &issuer)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("too large"));
        server.await.unwrap();
    }

    #[test]
    fn pkce_produces_correct_lengths() {
        let (verifier, challenge) = generate_pkce();
        assert!(verifier.len() >= 43 && verifier.len() <= 128);
        assert_eq!(challenge.len(), 43);
    }

    #[test]
    fn pkce_verifier_is_url_safe() {
        let (verifier, _) = generate_pkce();
        assert!(verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn authorization_url_contains_required_params() {
        let url = build_authorization_url("test_challenge", "test_state", 1455);
        assert!(url.contains("response_type=code"));
        assert!(url.contains(&format!("client_id={CLIENT_ID}")));
        assert!(url.contains("code_challenge=test_challenge"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=test_state"));
        assert!(url.contains("scope="));
        assert!(url.contains("originator=codex_cli_rs"));
    }

    #[test]
    fn extract_account_id_from_jwt() {
        let header = base64url_encode(b"{\"alg\":\"RS256\"}");
        let payload = base64url_encode(
            r#"{"sub":"user-123","chatgpt_account_id":"acct-456","exp":9999999999}"#.as_bytes(),
        );
        let sig = "sig";
        let token = format!("{header}.{payload}.{sig}");
        assert_eq!(extract_account_id(&token), Some("acct-456".to_string()));
    }

    #[test]
    fn extract_account_id_falls_back_to_sub() {
        let header = base64url_encode(b"{\"alg\":\"RS256\"}");
        let payload = base64url_encode(r#"{"sub":"user-789","exp":9999999999}"#.as_bytes());
        let sig = "sig";
        let token = format!("{header}.{payload}.{sig}");
        assert_eq!(extract_account_id(&token), Some("user-789".to_string()));
    }

    #[test]
    fn extract_account_id_supports_namespaced_codex_claim() {
        let header = base64url_encode(b"{\"alg\":\"RS256\"}");
        let payload = base64url_encode(
            r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"acct-nested"},"sub":"user-1","exp":9999999999}"#
                .as_bytes(),
        );
        let token = format!("{header}.{payload}.sig");

        assert_eq!(extract_account_id(&token).as_deref(), Some("acct-nested"));
    }

    #[test]
    fn tokens_valid_rejects_expired() {
        let header = base64url_encode(b"{\"alg\":\"RS256\"}");
        let payload = base64url_encode(r#"{"exp":1000000000}"#.as_bytes());
        let sig = "sig";
        let tokens = CodexTokens {
            access_token: format!("{header}.{payload}.{sig}"),
            id_token: "x.y.z".into(),
            refresh_token: "refresh".into(),
            account_id: None,
        };
        assert!(!tokens_valid(&tokens));
    }

    #[test]
    fn oauth_debug_output_redacts_all_credentials() {
        let tokens = CodexTokens {
            access_token: "access-secret".into(),
            id_token: "identity-secret".into(),
            refresh_token: "refresh-secret".into(),
            account_id: Some("account-secret".into()),
        };
        let rendered = format!("{tokens:?}");
        for secret in [
            "access-secret",
            "identity-secret",
            "refresh-secret",
            "account-secret",
        ] {
            assert!(!rendered.contains(secret));
        }
    }

    #[test]
    fn tokens_valid_accepts_future_expiry() {
        let header = base64url_encode(b"{\"alg\":\"RS256\"}");
        let payload = base64url_encode(r#"{"exp":9999999999}"#.as_bytes());
        let sig = "sig";
        let tokens = CodexTokens {
            access_token: format!("{header}.{payload}.{sig}"),
            id_token: "x.y.z".into(),
            refresh_token: "refresh".into(),
            account_id: None,
        };
        assert!(tokens_valid(&tokens));
    }

    #[test]
    fn redirect_uri_format() {
        assert_eq!(redirect_uri(1455), "http://localhost:1455/auth/callback");
    }
}
