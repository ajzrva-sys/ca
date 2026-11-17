use anyhow::Result;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The local consumer-chat bridge (`chatgpt-shim` → ChatGPT chat).
/// It receives the conversation the user wrote. It does not receive task-map
/// prompts, filename aliases, or tool-call protocol.
pub(crate) fn consumer_chat_shim(provider_id: &str) -> bool {
    provider_id == "chatgpt-shim"
}

pub(crate) fn skips_provider_filter(provider_id: &str) -> bool {
    provider_id == "openai-chatgpt" || consumer_chat_shim(provider_id)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ChatGptPlanTokens {
    pub client_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub account_sub: String,
    pub scopes: Vec<String>,
    pub ext_agent_host_id: String,
    pub expires_at: Option<i64>,
    pub email: Option<String>,
}

impl std::fmt::Debug for ChatGptPlanTokens {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ChatGptPlanTokens")
            .field("client_id", &self.client_id)
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field("id_token", &"[REDACTED]")
            .field("account_sub", &self.account_sub)
            .field("scopes", &self.scopes)
            .field("ext_agent_host_id", &self.ext_agent_host_id)
            .field("expires_at", &self.expires_at)
            .field("email", &self.email.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

pub struct ChatGptPlanStore {
    dir: PathBuf,
}

impl ChatGptPlanStore {
    pub fn from_dir(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Keychain account name for this store. File storage is the v1 source of truth.
    #[allow(dead_code)]
    pub fn keychain_service(&self) -> &'static str {
        "ca.chatgpt-plan-oauth"
    }

    pub fn load(&self) -> Result<Option<ChatGptPlanTokens>> {
        let path = self.file_path();
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path)?;
        let tokens: ChatGptPlanTokens = serde_json::from_slice(&bytes)?;
        Ok(Some(tokens))
    }

    pub fn save(&self, tokens: &ChatGptPlanTokens) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.file_path();
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(tokens)?;
        std::fs::write(&tmp, bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        let path = self.file_path();
        if path.is_file() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    fn file_path(&self) -> PathBuf {
        self.dir.join("chatgpt-plan.json")
    }
}

pub struct AuthorizationRequest {
    pub issued_client_id: Option<String>,
    pub ext_agent_host_id: String,
    pub redirect_uri: String,
    pub state: String,
    pub nonce: String,
    pub code_challenge: String,
    pub id_token_hint: Option<String>,
}

impl AuthorizationRequest {
    pub fn authorize_url(&self) -> String {
        let mut url = url::Url::parse("https://auth.openai.com/api/accounts/authorize")
            .expect("static authorize URL");
        {
            let mut query = url.query_pairs_mut();
            let client_id = self
                .issued_client_id
                .as_deref()
                .unwrap_or("dynamic_agent_client");
            query.append_pair("response_type", "code");
            query.append_pair("client_id", client_id);
            query.append_pair("redirect_uri", &self.redirect_uri);
            query.append_pair(
                "scope",
                "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct",
            );
            query.append_pair("resource", "https://api.openai.com/v1");
            query.append_pair("state", &self.state);
            query.append_pair("nonce", &self.nonce);
            query.append_pair("code_challenge", &self.code_challenge);
            query.append_pair("code_challenge_method", "S256");
            query.append_pair("ext_agent_host_id", &self.ext_agent_host_id);
            if self.issued_client_id.is_none() {
                query.append_pair("agent_name_hint", "CA");
            }
            if let Some(hint) = self.id_token_hint.as_deref() {
                query.append_pair("id_token_hint", hint);
            }
        }
        url.into()
    }
}

pub struct PendingAuthorization {
    pub issued_client_id: Option<String>,
    pub state: String,
    /// Checked when the ID token comes back. Kept on the pending request until then.
    #[allow(dead_code)]
    pub nonce: String,
    pub code_verifier: String,
    /// The loopback URL this authorization is waiting on.
    #[allow(dead_code)]
    pub redirect_uri: String,
}

#[derive(Debug)]
pub struct AuthorizationCallback {
    pub code: String,
    pub client_id: String,
}

impl PendingAuthorization {
    pub fn accept_callback(&self, query: &str) -> Result<AuthorizationCallback> {
        let params: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect();
        let value = |name: &str| {
            params
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        if value("state") != Some(self.state.as_str()) {
            anyhow::bail!("OAuth callback state did not match the pending authorization");
        }
        let Some(code) = value("code").filter(|code| !code.is_empty()) else {
            anyhow::bail!("OAuth callback did not include an authorization code");
        };
        let callback_client_id = value("client_id");
        let client_id = match (self.issued_client_id.as_deref(), callback_client_id) {
            (_, Some("dynamic_agent_client")) => {
                anyhow::bail!("callback issued client_id must not be dynamic_agent_client")
            }
            (None, Some(issued)) if !issued.is_empty() => issued.to_string(),
            (None, _) => {
                anyhow::bail!("new registration callback did not include an issued client_id")
            }
            (Some(pending), Some(issued)) if issued != pending => {
                anyhow::bail!("callback issued client_id did not match the pending registration")
            }
            (Some(pending), _) => pending.to_string(),
        };
        Ok(AuthorizationCallback {
            code: code.to_string(),
            client_id,
        })
    }
}

pub const TOKEN_URL: &str = "https://auth.openai.com/api/accounts/oauth/token";

#[cfg(test)]
std::thread_local! {
    static TOKEN_URL_OVERRIDE: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

pub fn token_endpoint() -> String {
    #[cfg(test)]
    if let Some(url) = TOKEN_URL_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return url;
    }
    TOKEN_URL.to_string()
}

#[cfg(test)]
pub fn set_token_url_for_test(url: Option<String>) {
    TOKEN_URL_OVERRIDE.with(|slot| *slot.borrow_mut() = url);
}

pub fn token_exchange_body(
    client_id: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("client_id", client_id)
        .append_pair("code", code)
        .append_pair("code_verifier", code_verifier)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("resource", "https://api.openai.com/v1")
        .finish()
}

pub fn plan_use_enabled(scopes: &[impl AsRef<str>]) -> bool {
    scopes
        .iter()
        .any(|scope| scope.as_ref() == "chatgpt.tokens.use.direct")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatGptPlanErrorVerdict {
    pub is_usage_limit: bool,
    pub allow_provider_fallback: bool,
    pub user_message: String,
}

pub fn classify_chatgpt_plan_error(status: u16, body: &str) -> ChatGptPlanErrorVerdict {
    let code = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/code")
                .or_else(|| value.pointer("/response/error/code"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
    match (status, code.as_deref()) {
        (429, Some("subscription_sharing_usage_limit_exceeded")) => ChatGptPlanErrorVerdict {
            is_usage_limit: true,
            allow_provider_fallback: false,
            user_message: "ChatGPT plan usage limit reached. Review usage at https://chatgpt.com/settings/usage".into(),
        },
        (503, Some("subscription_sharing_usage_unavailable")) => ChatGptPlanErrorVerdict {
            is_usage_limit: true,
            allow_provider_fallback: false,
            user_message: "ChatGPT plan usage is temporarily unavailable. Try again later.".into(),
        },
        _ => ChatGptPlanErrorVerdict {
            is_usage_limit: false,
            allow_provider_fallback: false,
            user_message: format!("ChatGPT plan request failed (HTTP {status})"),
        },
    }
}

pub fn load_or_create_host_id(dir: &std::path::Path) -> Result<String> {
    let path = dir.join("chatgpt-plan-host-id");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if trimmed.starts_with("urn:uuid:") && !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    std::fs::create_dir_all(dir)?;
    let host_id = format!("urn:uuid:{}", uuid::Uuid::new_v4());
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &host_id)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(tmp, path)?;
    Ok(host_id)
}

pub fn access_token_expired(tokens: &ChatGptPlanTokens, now_unix: i64) -> bool {
    tokens.expires_at.is_some_and(|expiry| expiry <= now_unix)
}

fn expiry_from_response(json: &serde_json::Value) -> Option<i64> {
    json.get("expires_in")
        .and_then(serde_json::Value::as_i64)
        .map(|seconds| chrono::Utc::now().timestamp().saturating_add(seconds))
}

pub const SIGN_IN_AGAIN: &str = "ChatGPT plan sign-in expired. Continue with ChatGPT again.";

pub async fn refresh_plan_tokens_at(
    url: &str,
    tokens: &ChatGptPlanTokens,
) -> Result<ChatGptPlanTokens> {
    let body = token_refresh_body(&tokens.client_id, &tokens.refresh_token);
    let response = reqwest::Client::new()
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?;
    let status = response.status();
    let json: serde_json::Value = response.json().await?;
    if json.get("error").and_then(serde_json::Value::as_str) == Some("invalid_grant") {
        anyhow::bail!(SIGN_IN_AGAIN);
    }
    if !status.is_success() {
        anyhow::bail!("token refresh returned HTTP {}", status.as_u16());
    }
    let access_token = json
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("token refresh response missing access_token"))?
        .to_string();
    let refresh_token = json
        .get("refresh_token")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .unwrap_or(&tokens.refresh_token)
        .to_string();
    let id_token = json
        .get("id_token")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .unwrap_or(&tokens.id_token)
        .to_string();
    let scopes = match json.get("scope").and_then(serde_json::Value::as_str) {
        Some(scope) if !scope.is_empty() => scope.split_whitespace().map(str::to_string).collect(),
        _ => tokens.scopes.clone(),
    };
    if !plan_use_enabled(&scopes) {
        anyhow::bail!(SIGN_IN_AGAIN);
    }
    Ok(ChatGptPlanTokens {
        client_id: tokens.client_id.clone(),
        access_token,
        refresh_token,
        id_token,
        account_sub: tokens.account_sub.clone(),
        scopes,
        ext_agent_host_id: tokens.ext_agent_host_id.clone(),
        expires_at: expiry_from_response(&json),
        email: tokens.email.clone(),
    })
}

pub fn token_refresh_body(client_id: &str, refresh_token: &str) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "refresh_token")
        .append_pair("client_id", client_id)
        .append_pair("refresh_token", refresh_token)
        .append_pair("resource", "https://api.openai.com/v1")
        .finish()
}

pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}/auth/callback")
}

pub fn generate_pkce() -> (String, String) {
    let rng = ring::rand::SystemRandom::new();
    let mut bytes = [0u8; 32];
    ring::rand::SecureRandom::fill(&rng, &mut bytes).expect("system RNG is available");
    let verifier = Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes);
    let digest = ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes());
    let challenge = Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        digest.as_ref(),
    );
    (verifier, challenge)
}

pub async fn receive_callback(listener: tokio::net::TcpListener) -> Result<String> {
    let (mut stream, _) = listener.accept().await?;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await?;
    buf.truncate(n);
    let request = String::from_utf8_lossy(&buf);
    let first_line = request.lines().next().unwrap_or("");
    let path = first_line.split_whitespace().nth(1).unwrap_or("");
    let query = path
        .split_once('?')
        .map(|(_, query)| query.to_string())
        .unwrap_or_default();
    let body = "CA signed in. You can close this tab.";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    Ok(query)
}

const CALLBACK_TIMEOUT_SECS: u64 = 300;

async fn bind_callback_listener() -> Result<(tokio::net::TcpListener, u16)> {
    for port in [1455_u16, 1457] {
        if let Ok(listener) =
            tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await
        {
            return Ok((listener, port));
        }
    }
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/open")
            .arg(url)
            .status()?;
        if status.success() {
            Ok(())
        } else {
            anyhow::bail!("open exited with {status}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let status = std::process::Command::new("xdg-open").arg(url).status()?;
        if status.success() {
            Ok(())
        } else {
            anyhow::bail!("xdg-open exited with {status}");
        }
    }
}

pub async fn authorize_browser_candidate_with_progress(
    progress: impl FnOnce(String) + Send,
) -> Result<ChatGptPlanTokens> {
    let dir = default_plan_home();
    let host_id = load_or_create_host_id(&dir)?;
    let (code_verifier, code_challenge) = generate_pkce();
    let state = Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, {
        let rng = ring::rand::SystemRandom::new();
        let mut bytes = [0u8; 16];
        ring::rand::SecureRandom::fill(&rng, &mut bytes).expect("system RNG is available");
        bytes
    });
    let nonce = Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, {
        let rng = ring::rand::SystemRandom::new();
        let mut bytes = [0u8; 16];
        ring::rand::SecureRandom::fill(&rng, &mut bytes).expect("system RNG is available");
        bytes
    });
    let store = ChatGptPlanStore::from_dir(&dir);
    let existing = store.load()?;
    let (listener, port) = bind_callback_listener().await?;
    let redirect = redirect_uri(port);
    let req = AuthorizationRequest {
        issued_client_id: existing.as_ref().map(|tokens| tokens.client_id.clone()),
        ext_agent_host_id: host_id.clone(),
        redirect_uri: redirect.clone(),
        state: state.clone(),
        nonce: nonce.clone(),
        code_challenge,
        id_token_hint: existing.as_ref().map(|tokens| tokens.id_token.clone()),
    };
    let auth_url = req.authorize_url();
    progress(auth_url.clone());
    let url_for_browser = auth_url;
    tokio::task::spawn_blocking(move || open_browser(&url_for_browser)).await??;
    let query = tokio::time::timeout(
        std::time::Duration::from_secs(CALLBACK_TIMEOUT_SECS),
        receive_callback(listener),
    )
    .await
    .map_err(|_| anyhow::anyhow!("OAuth callback timed out"))??;
    let pending = PendingAuthorization {
        issued_client_id: existing.as_ref().map(|tokens| tokens.client_id.clone()),
        state,
        nonce: nonce.clone(),
        code_verifier,
        redirect_uri: redirect.clone(),
    };
    let callback = pending.accept_callback(&query)?;
    let tokens = exchange_code_for_tokens(
        &callback.client_id,
        &callback.code,
        &pending.code_verifier,
        &redirect,
    )
    .await?;
    if !plan_use_enabled(&tokens.scopes) {
        anyhow::bail!("ChatGPT plan use is not enabled for this sign-in");
    }
    let sub = validate_id_token_claims(
        &tokens.id_token,
        &IdTokenChecks {
            nonce,
            issued_client_id: callback.client_id.clone(),
            expected_sub: existing.as_ref().map(|tokens| tokens.account_sub.clone()),
        },
    )?;
    Ok(ChatGptPlanTokens {
        client_id: callback.client_id,
        account_sub: sub,
        ext_agent_host_id: host_id,
        ..tokens
    })
}

fn default_plan_home() -> PathBuf {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    config_home.join("ca")
}

async fn exchange_code_for_tokens(
    client_id: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<ChatGptPlanTokens> {
    let body = token_exchange_body(client_id, code, code_verifier, redirect_uri);
    let response = reqwest::Client::new()
        .post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?;
    if !response.status().is_success() {
        anyhow::bail!("token exchange returned HTTP {}", response.status());
    }
    let json: serde_json::Value = response.json().await?;
    let access_token = json
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("token response missing access_token"))?
        .to_string();
    let refresh_token = json
        .get("refresh_token")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let id_token = json
        .get("id_token")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let scopes = json
        .get("scope")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .split_whitespace()
        .map(str::to_string)
        .collect();
    Ok(ChatGptPlanTokens {
        client_id: client_id.to_string(),
        access_token,
        refresh_token,
        id_token,
        account_sub: String::new(),
        scopes,
        ext_agent_host_id: String::new(),
        expires_at: expiry_from_response(&json),
        email: None,
    })
}

pub struct IdTokenChecks {
    pub nonce: String,
    pub issued_client_id: String,
    pub expected_sub: Option<String>,
}

pub fn validate_id_token_claims(id_token: &str, checks: &IdTokenChecks) -> Result<String> {
    let payload = id_token
        .split('.')
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("ID token is not a JWT"))?;
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, payload)?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes)?;
    let issuer = claims.get("iss").and_then(serde_json::Value::as_str);
    if issuer != Some("https://auth.openai.com") {
        anyhow::bail!("ID token issuer did not match https://auth.openai.com");
    }
    let audience = claims.get("aud").and_then(serde_json::Value::as_str);
    if audience != Some(checks.issued_client_id.as_str()) {
        anyhow::bail!("ID token audience did not match the issued client id");
    }
    let nonce = claims.get("nonce").and_then(serde_json::Value::as_str);
    if nonce != Some(checks.nonce.as_str()) {
        anyhow::bail!("ID token nonce did not match the pending authorization");
    }
    let sub = claims
        .get("sub")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("ID token is missing sub"))?;
    if let Some(expected) = checks.expected_sub.as_deref() {
        if sub != expected {
            anyhow::bail!("ID token subject did not match the stored ChatGPT account");
        }
    }
    Ok(sub.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ca-siwc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn chatgpt_plan_store_does_not_read_codex_tokens() {
        let root = tmp();
        std::fs::write(
            root.join("codex-tokens.json"),
            "{\"access_token\":\"CODEX_SECRET\"}",
        )
        .unwrap();
        let store = ChatGptPlanStore::from_dir(root.join("chatgpt-plan"));
        assert!(store.load().unwrap().is_none());
        assert_eq!(store.keychain_service(), "ca.chatgpt-plan-oauth");
        assert_ne!(store.keychain_service(), "ca.codex-oauth");
    }

    #[test]
    fn chatgpt_plan_debug_redacts_secrets() {
        let tokens = ChatGptPlanTokens {
            client_id: "oaiapp_test".into(),
            access_token: "ACCESS_SECRET".into(),
            refresh_token: "REFRESH_SECRET".into(),
            id_token: "ID_SECRET".into(),
            account_sub: "sub_test".into(),
            scopes: vec!["chatgpt.tokens.use.direct".into()],
            ext_agent_host_id: "urn:uuid:00000000-0000-0000-0000-000000000001".into(),
            expires_at: None,
            email: None,
        };
        let text = format!("{tokens:?}");
        assert!(!text.contains("ACCESS_SECRET"));
        assert!(!text.contains("REFRESH_SECRET"));
        assert!(!text.contains("ID_SECRET"));
        assert!(text.contains("[REDACTED]"));
    }

    #[test]
    fn chatgpt_plan_authorize_url_uses_dynamic_client_and_direct_scope() {
        let req = AuthorizationRequest {
            issued_client_id: None,
            ext_agent_host_id: "urn:uuid:11111111-1111-1111-1111-111111111111".into(),
            redirect_uri: "http://127.0.0.1:1455/auth/callback".into(),
            state: "state-test".into(),
            nonce: "nonce-test".into(),
            code_challenge: "challenge-test".into(),
            id_token_hint: None,
        };
        let url = req.authorize_url();
        assert!(url.starts_with("https://auth.openai.com/api/accounts/authorize?"));
        assert!(url.contains("client_id=dynamic_agent_client"));
        assert!(url.contains("agent_name_hint=CA"));
        assert!(url.contains("ext_agent_host_id="));
        assert!(url.contains("resource=https%3A%2F%2Fapi.openai.com%2Fv1"));
        assert!(url.contains("chatgpt.tokens.use.direct"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(!url.contains("localhost"));
        assert!(!url.contains("app_EMoamEEZ73f0CkXaXp7hrann"));
    }

    #[test]
    fn chatgpt_plan_returning_authorize_url_omits_agent_name_hint() {
        let req = AuthorizationRequest {
            issued_client_id: Some("oaiapp_saved".into()),
            ext_agent_host_id: "urn:uuid:11111111-1111-1111-1111-111111111111".into(),
            redirect_uri: "http://127.0.0.1:1457/auth/callback".into(),
            state: "state-test".into(),
            nonce: "nonce-test".into(),
            code_challenge: "challenge-test".into(),
            id_token_hint: Some("expired-id-token".into()),
        };
        let url = req.authorize_url();
        assert!(url.contains("client_id=oaiapp_saved"));
        assert!(!url.contains("dynamic_agent_client"));
        assert!(!url.contains("agent_name_hint"));
        assert!(url.contains("id_token_hint="));
    }

    #[test]
    fn chatgpt_plan_new_registration_keeps_issued_client_id() {
        let pending = PendingAuthorization {
            issued_client_id: None,
            state: "abc".into(),
            nonce: "n".into(),
            code_verifier: "verifier".into(),
            redirect_uri: "http://127.0.0.1:1455/auth/callback".into(),
        };
        let cb = pending
            .accept_callback("code=tok&state=abc&client_id=oaiapp_issued")
            .unwrap();
        assert_eq!(cb.code, "tok");
        assert_eq!(cb.client_id, "oaiapp_issued");
    }

    #[test]
    fn chatgpt_plan_rejects_dynamic_agent_client_as_issued_id() {
        let pending = PendingAuthorization {
            issued_client_id: None,
            state: "abc".into(),
            nonce: "n".into(),
            code_verifier: "verifier".into(),
            redirect_uri: "http://127.0.0.1:1455/auth/callback".into(),
        };
        let err = pending
            .accept_callback("code=tok&state=abc&client_id=dynamic_agent_client")
            .unwrap_err();
        assert!(format!("{err}").contains("issued"));
    }

    #[test]
    fn chatgpt_plan_state_mismatch_is_rejected() {
        let pending = PendingAuthorization {
            issued_client_id: None,
            state: "abc".into(),
            nonce: "n".into(),
            code_verifier: "verifier".into(),
            redirect_uri: "http://127.0.0.1:1455/auth/callback".into(),
        };
        assert!(pending.accept_callback("code=tok&state=other").is_err());
    }

    #[test]
    fn chatgpt_plan_exchange_body_has_no_client_secret() {
        let body = token_exchange_body(
            "oaiapp_issued",
            "auth-code",
            "verifier",
            "http://127.0.0.1:1455/auth/callback",
        );
        assert!(body.contains("grant_type=authorization_code"));
        assert!(body.contains("client_id=oaiapp_issued"));
        assert!(!body.contains("client_secret"));
        assert!(body.contains("resource=https%3A%2F%2Fapi.openai.com%2Fv1"));
    }

    #[test]
    fn chatgpt_plan_requires_direct_use_scope() {
        assert!(plan_use_enabled(&["openid", "chatgpt.tokens.use.direct"]));
        assert!(!plan_use_enabled(&["openid", "profile"]));
    }

    #[test]
    fn chatgpt_plan_usage_limit_does_not_activate_fallback() {
        let verdict = classify_chatgpt_plan_error(
            429,
            r#"{"error":{"code":"subscription_sharing_usage_limit_exceeded"}}"#,
        );
        assert!(verdict.is_usage_limit);
        assert!(!verdict.allow_provider_fallback);
        assert!(verdict.user_message.contains("ChatGPT plan"));
    }

    #[test]
    fn chatgpt_plan_host_id_is_stable_per_ca_home() {
        let dir = tmp();
        let a = load_or_create_host_id(&dir).unwrap();
        let b = load_or_create_host_id(&dir).unwrap();
        assert_eq!(a, b);
        assert!(a.starts_with("urn:uuid:"));
    }

    #[test]
    fn chatgpt_plan_refresh_body_uses_issued_client_and_resource() {
        let body = token_refresh_body("oaiapp_issued", "refresh-secret");
        assert!(body.contains("grant_type=refresh_token"));
        assert!(body.contains("client_id=oaiapp_issued"));
        assert!(body.contains("resource=https%3A%2F%2Fapi.openai.com%2Fv1"));
        assert!(!body.contains("dynamic_agent_client"));
        assert!(!body.contains("scope="));
        assert!(!body.contains("client_secret"));
    }

    fn unsigned_id_token(claims: &str) -> String {
        let header = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            br#"{"alg":"none"}"#,
        );
        let payload = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            claims.as_bytes(),
        );
        format!("{header}.{payload}.sig")
    }

    #[test]
    fn chatgpt_plan_id_token_rejects_issuer_mismatch() {
        let token = unsigned_id_token(
            r#"{"iss":"https://example.invalid","aud":"oaiapp_issued","nonce":"n","sub":"sub_test"}"#,
        );
        let err = validate_id_token_claims(
            &token,
            &IdTokenChecks {
                nonce: "n".into(),
                issued_client_id: "oaiapp_issued".into(),
                expected_sub: None,
            },
        )
        .unwrap_err();
        assert!(format!("{err}").contains("issuer"));
    }

    #[test]
    fn chatgpt_plan_redirect_uri_is_loopback_auth_callback() {
        assert_eq!(redirect_uri(1455), "http://127.0.0.1:1455/auth/callback");
        assert_eq!(redirect_uri(1457), "http://127.0.0.1:1457/auth/callback");
        assert!(!redirect_uri(1455).contains("localhost"));
    }

    #[test]
    fn chatgpt_plan_pkce_challenge_is_s256() {
        let (verifier, challenge) = generate_pkce();
        assert_ne!(verifier, challenge);
        assert!(!challenge.contains('='));
        let digest = ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes());
        let expected = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            digest.as_ref(),
        );
        assert_eq!(challenge, expected);
    }

    #[tokio::test]
    async fn chatgpt_plan_receive_callback_keeps_issued_client_id() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(receive_callback(listener));
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        use tokio::io::AsyncWriteExt;
        stream
            .write_all(
                format!(
                    "GET /auth/callback?code=tok&state=abc&client_id=oaiapp_issued HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let query = handle.await.unwrap().unwrap();
        let pending = PendingAuthorization {
            issued_client_id: None,
            state: "abc".into(),
            nonce: "n".into(),
            code_verifier: "verifier".into(),
            redirect_uri: redirect_uri(port),
        };
        let cb = pending.accept_callback(&query).unwrap();
        assert_eq!(cb.code, "tok");
        assert_eq!(cb.client_id, "oaiapp_issued");
    }
}
