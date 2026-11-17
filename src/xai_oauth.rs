//! Device-code sign-in for the xAI SuperGrok / X Premium+ subscription.
//!
//! The public client id is the allowlisted Grok CLI client. It is not a secret.
//! Refresh tokens stay in an owner-only file under the ca config directory.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const DEVICE_CODE_URL: &str = "https://auth.x.ai/oauth2/device/code";
pub const TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";
/// Allowlisted public client. Not a credential.
pub const PUBLIC_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub const SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";

const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
const MAX_BODY_BYTES: usize = 64 * 1024;
const REFRESH_SKEW_SECS: i64 = 120;
const MAX_POLL_SECS: u64 = 900;

#[derive(Debug)]
pub struct XaiOauthError {
    message: String,
    invalid_grant: bool,
}

impl XaiOauthError {
    fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            invalid_grant: false,
        }
    }

    fn invalid_grant() -> Self {
        Self {
            message: "Sign in to xAI Grok again".into(),
            invalid_grant: true,
        }
    }

    pub const fn is_invalid_grant(&self) -> bool {
        self.invalid_grant
    }
}

impl std::fmt::Display for XaiOauthError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for XaiOauthError {}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct XaiOauthTokens {
    pub client_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: Option<i64>,
}

impl std::fmt::Debug for XaiOauthTokens {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("XaiOauthTokens")
            .field("client_id", &self.client_id)
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Clone)]
pub struct DeviceAuthorization {
    pub verification_url: String,
    pub user_code: String,
    client_id: String,
    device_code: String,
    pub interval_secs: u64,
    pub expires_in_secs: u64,
}

impl std::fmt::Debug for DeviceAuthorization {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeviceAuthorization")
            .field("verification_url", &self.verification_url)
            .field("user_code", &self.user_code)
            .field("client_id", &self.client_id)
            .field("device_code", &"[REDACTED]")
            .field("interval_secs", &self.interval_secs)
            .field("expires_in_secs", &self.expires_in_secs)
            .finish()
    }
}

pub struct XaiOauthStore {
    dir: PathBuf,
}

impl XaiOauthStore {
    pub fn from_dir(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn default() -> Self {
        Self::from_dir(default_config_dir())
    }

    pub fn signed_in(&self) -> bool {
        self.load().ok().flatten().is_some_and(|tokens| {
            !tokens.access_token.is_empty() && !tokens.refresh_token.is_empty()
        })
    }

    pub fn load(&self) -> Result<Option<XaiOauthTokens>, XaiOauthError> {
        let path = self.file_path();
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path).map_err(|error| {
            XaiOauthError::message(format!("could not read xAI sign-in: {error}"))
        })?;
        if bytes.len() > MAX_BODY_BYTES {
            return Err(XaiOauthError::message("xAI sign-in file is too large"));
        }
        let tokens: XaiOauthTokens = serde_json::from_slice(&bytes)
            .map_err(|_| XaiOauthError::message("xAI sign-in file is invalid"))?;
        if tokens.access_token.is_empty() || tokens.refresh_token.is_empty() {
            return Err(XaiOauthError::message("xAI sign-in file is invalid"));
        }
        Ok(Some(tokens))
    }

    pub fn save(&self, tokens: &XaiOauthTokens) -> Result<(), XaiOauthError> {
        std::fs::create_dir_all(&self.dir).map_err(|error| {
            XaiOauthError::message(format!(
                "could not create the xAI sign-in directory: {error}"
            ))
        })?;
        let path = self.file_path();
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(tokens)
            .map_err(|_| XaiOauthError::message("could not store the xAI sign-in"))?;
        std::fs::write(&tmp, bytes).map_err(|error| {
            XaiOauthError::message(format!("could not write the xAI sign-in: {error}"))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)).map_err(
                |error| {
                    XaiOauthError::message(format!("could not protect the xAI sign-in: {error}"))
                },
            )?;
        }
        std::fs::rename(&tmp, &path).map_err(|error| {
            XaiOauthError::message(format!("could not store the xAI sign-in: {error}"))
        })?;
        Ok(())
    }

    pub fn clear(&self) -> Result<(), XaiOauthError> {
        let path = self.file_path();
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|error| {
                XaiOauthError::message(format!("could not remove the xAI sign-in: {error}"))
            })?;
        }
        Ok(())
    }

    fn file_path(&self) -> PathBuf {
        self.dir.join("xai-oauth.json")
    }
}

pub fn access_expired(tokens: &XaiOauthTokens, now_unix: i64) -> bool {
    match tokens.expires_at {
        Some(expires_at) => expires_at <= now_unix.saturating_add(REFRESH_SKEW_SECS),
        None => true,
    }
}

pub async fn request_device_authorization() -> Result<DeviceAuthorization, XaiOauthError> {
    let client = auth_client()?;
    request_device_authorization_at(&client, DEVICE_CODE_URL, &client_id()).await
}

pub async fn complete_device_authorization(
    authorization: &DeviceAuthorization,
) -> Result<XaiOauthTokens, XaiOauthError> {
    let client = auth_client()?;
    poll_device_token(
        &client,
        TOKEN_URL,
        &authorization.client_id,
        &authorization.device_code,
        Duration::from_secs(authorization.interval_secs),
        Instant::now() + Duration::from_secs(authorization.expires_in_secs.min(MAX_POLL_SECS)),
    )
    .await
}

pub async fn resolve_access_token(store: &XaiOauthStore) -> Result<String, XaiOauthError> {
    let Some(tokens) = store.load()? else {
        return Err(XaiOauthError::message("xAI Grok is not signed in"));
    };
    let now = unix_now();
    if !access_expired(&tokens, now) {
        return Ok(tokens.access_token);
    }
    match refresh_tokens_at(&auth_client()?, TOKEN_URL, &tokens).await {
        Ok(fresh) => {
            store.save(&fresh)?;
            Ok(fresh.access_token)
        }
        Err(error) if error.is_invalid_grant() => {
            let _ = store.clear();
            Err(error)
        }
        Err(error) => Err(error),
    }
}

pub async fn request_device_authorization_at(
    client: &reqwest::Client,
    device_url: &str,
    client_id: &str,
) -> Result<DeviceAuthorization, XaiOauthError> {
    let response = client
        .post(device_url)
        .form(&[("client_id", client_id), ("scope", SCOPE)])
        .send()
        .await
        .map_err(|_| XaiOauthError::message("could not start xAI sign-in"))?;
    if !response.status().is_success() {
        return Err(XaiOauthError::message(format!(
            "xAI sign-in returned HTTP {}",
            response.status().as_u16()
        )));
    }
    let body: DeviceCodeBody = read_json(response, "xAI sign-in").await?;
    let verification_url = accepted_verification_url(
        body.verification_uri_complete.as_deref(),
        body.verification_uri.as_deref(),
    )?;
    let user_code = body.user_code.unwrap_or_default();
    let device_code = body.device_code.unwrap_or_default();
    if user_code.is_empty()
        || device_code.is_empty()
        || user_code.len() > 256
        || device_code.len() > 4096
    {
        return Err(XaiOauthError::message("xAI sign-in response was invalid"));
    }
    let expires_in_secs = body
        .expires_in
        .unwrap_or(MAX_POLL_SECS)
        .clamp(1, MAX_POLL_SECS);
    Ok(DeviceAuthorization {
        verification_url,
        user_code,
        client_id: client_id.to_string(),
        device_code,
        interval_secs: poll_interval(body.interval),
        expires_in_secs,
    })
}

pub async fn refresh_tokens_at(
    client: &reqwest::Client,
    token_url: &str,
    tokens: &XaiOauthTokens,
) -> Result<XaiOauthTokens, XaiOauthError> {
    let response = client
        .post(token_url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", tokens.refresh_token.as_str()),
            ("client_id", tokens.client_id.as_str()),
        ])
        .send()
        .await
        .map_err(|_| XaiOauthError::message("could not refresh the xAI sign-in"))?;
    let status = response.status();
    if status.as_u16() == 400 {
        let code = error_code(response).await;
        if code == "invalid_grant" {
            return Err(XaiOauthError::invalid_grant());
        }
        return Err(XaiOauthError::message("could not refresh the xAI sign-in"));
    }
    if !status.is_success() {
        return Err(XaiOauthError::message(format!(
            "xAI token refresh returned HTTP {}",
            status.as_u16()
        )));
    }
    let body: TokenBody = read_json(response, "xAI token refresh").await?;
    tokens_from_body(
        &tokens.client_id,
        &body,
        Some(tokens.refresh_token.as_str()),
    )
}

async fn poll_device_token(
    client: &reqwest::Client,
    token_url: &str,
    client_id: &str,
    device_code: &str,
    mut interval: Duration,
    deadline: Instant,
) -> Result<XaiOauthTokens, XaiOauthError> {
    loop {
        if Instant::now() >= deadline {
            return Err(XaiOauthError::message("xAI sign-in expired"));
        }
        tokio::time::sleep(interval).await;
        let response = client
            .post(token_url)
            .form(&[
                ("grant_type", DEVICE_GRANT),
                ("device_code", device_code),
                ("client_id", client_id),
            ])
            .send()
            .await
            .map_err(|_| XaiOauthError::message("could not finish xAI sign-in"))?;
        let status = response.status();
        if status.is_success() {
            let body: TokenBody = read_json(response, "xAI sign-in").await?;
            return tokens_from_body(client_id, &body, None);
        }
        if status.as_u16() != 400 {
            return Err(XaiOauthError::message(format!(
                "xAI sign-in returned HTTP {}",
                status.as_u16()
            )));
        }
        match error_code(response).await.as_str() {
            "authorization_pending" => {}
            "slow_down" => {
                interval = interval
                    .saturating_add(Duration::from_secs(5))
                    .min(Duration::from_secs(15));
            }
            "access_denied" => return Err(XaiOauthError::message("xAI sign-in was denied")),
            "expired_token" => return Err(XaiOauthError::message("xAI sign-in expired")),
            "invalid_grant" => return Err(XaiOauthError::invalid_grant()),
            _ => return Err(XaiOauthError::message("xAI sign-in failed")),
        }
    }
}

fn tokens_from_body(
    client_id: &str,
    body: &TokenBody,
    previous_refresh: Option<&str>,
) -> Result<XaiOauthTokens, XaiOauthError> {
    let access_token = body.access_token.clone().unwrap_or_default();
    let refresh_token = body
        .refresh_token
        .clone()
        .filter(|token| !token.is_empty())
        .or_else(|| previous_refresh.map(str::to_owned))
        .unwrap_or_default();
    if access_token.is_empty() || refresh_token.is_empty() || access_token.len() > 16_384 {
        return Err(XaiOauthError::message("xAI sign-in response was invalid"));
    }
    let expires_in = body.expires_in.unwrap_or(3600).clamp(60, 86_400);
    Ok(XaiOauthTokens {
        client_id: client_id.to_string(),
        access_token,
        refresh_token,
        expires_at: Some(unix_now().saturating_add(i64::try_from(expires_in).unwrap_or(3600))),
    })
}

fn accepted_verification_url(
    complete: Option<&str>,
    plain: Option<&str>,
) -> Result<String, XaiOauthError> {
    for candidate in [complete, plain].into_iter().flatten() {
        if let Some(url) = allowed_verification_url(candidate) {
            return Ok(url);
        }
    }
    Err(XaiOauthError::message(
        "xAI sign-in returned an unexpected verification URL",
    ))
}

fn allowed_verification_url(value: &str) -> Option<String> {
    let url = url::Url::parse(value).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    match url.host_str() {
        Some("auth.x.ai" | "accounts.x.ai") => Some(url.to_string()),
        _ => None,
    }
}

fn poll_interval(value: Option<u64>) -> u64 {
    match value {
        Some(0) | None => 5,
        Some(seconds) => seconds.clamp(1, 15),
    }
}

fn client_id() -> String {
    std::env::var("CA_XAI_OAUTH_CLIENT_ID")
        .ok()
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .unwrap_or_else(|| PUBLIC_CLIENT_ID.to_string())
}

fn auth_client() -> Result<reqwest::Client, XaiOauthError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| XaiOauthError::message("could not start the xAI sign-in client"))
}

async fn read_json<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
    label: &str,
) -> Result<T, XaiOauthError> {
    let bytes = response
        .bytes()
        .await
        .map_err(|_| XaiOauthError::message(format!("{label} response was unreadable")))?;
    if bytes.len() > MAX_BODY_BYTES {
        return Err(XaiOauthError::message(format!(
            "{label} response was too large"
        )));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| XaiOauthError::message(format!("{label} response was invalid")))
}

async fn error_code(response: reqwest::Response) -> String {
    let Ok(bytes) = response.bytes().await else {
        return String::new();
    };
    if bytes.len() > MAX_BODY_BYTES {
        return String::new();
    }
    serde_json::from_slice::<ErrorBody>(&bytes)
        .ok()
        .and_then(|body| body.error)
        .unwrap_or_default()
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn default_config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ca")
}

#[derive(Deserialize)]
struct DeviceCodeBody {
    device_code: Option<String>,
    user_code: Option<String>,
    verification_uri: Option<String>,
    verification_uri_complete: Option<String>,
    expires_in: Option<u64>,
    interval: Option<u64>,
}

#[derive(Deserialize)]
struct TokenBody {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_urls_stay_on_the_xai_sign_in_hosts() {
        assert!(allowed_verification_url("http://auth.x.ai/oauth2/device").is_none());
        assert!(allowed_verification_url("https://evil.example/auth.x.ai").is_none());
        assert!(allowed_verification_url("https://auth.x.ai.evil/device").is_none());
        assert!(
            allowed_verification_url("https://accounts.x.ai/oauth2/device?user_code=ABCD")
                .is_some()
        );
    }

    #[test]
    fn stored_sign_in_is_owner_only_and_debug_hides_tokens() {
        let temp = tempfile::tempdir().unwrap();
        let store = XaiOauthStore::from_dir(temp.path());
        let tokens = XaiOauthTokens {
            client_id: PUBLIC_CLIENT_ID.into(),
            access_token: "access-secret".into(),
            refresh_token: "refresh-secret".into(),
            expires_at: Some(9_999_999_999),
        };
        store.save(&tokens).unwrap();
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.access_token, "access-secret");
        let rendered = format!("{tokens:?}");
        assert!(!rendered.contains("access-secret"));
        assert!(!rendered.contains("refresh-secret"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(store.file_path())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(!access_expired(&tokens, 1_000));
        assert!(access_expired(&tokens, 9_999_999_999));
    }

    #[tokio::test]
    async fn device_sign_in_polls_until_the_token_and_keeps_a_missing_refresh_token() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let mut polls = 0_u8;
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let mut buffer = vec![0_u8; 4096];
                let Ok(count) = tokio::io::AsyncReadExt::read(&mut socket, &mut buffer).await
                else {
                    break;
                };
                let request = String::from_utf8_lossy(&buffer[..count]);
                let body = if request.starts_with("POST /device ") {
                    r#"{"device_code":"device-secret","user_code":"ABCD-EFGH","verification_uri":"https://auth.x.ai/oauth2/device","verification_uri_complete":"https://accounts.x.ai/oauth2/device?user_code=ABCD-EFGH","expires_in":30,"interval":1}"#
                } else if request.contains("grant_type=refresh_token") {
                    r#"{"access_token":"access-2","expires_in":120}"#
                } else {
                    polls += 1;
                    if polls == 1 {
                        r#"{"error":"authorization_pending"}"#
                    } else {
                        r#"{"access_token":"access-1","refresh_token":"refresh-1","expires_in":120}"#
                    }
                };
                let status = if body.contains("authorization_pending") {
                    "400 Bad Request"
                } else {
                    "200 OK"
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = tokio::io::AsyncWriteExt::write_all(&mut socket, response.as_bytes()).await;
            }
        });
        let client = auth_client().unwrap();
        let device_url = format!("http://{address}/device");
        let token_url = format!("http://{address}/token");
        let authorization = request_device_authorization_at(&client, &device_url, PUBLIC_CLIENT_ID)
            .await
            .unwrap();
        assert_eq!(authorization.user_code, "ABCD-EFGH");
        assert!(authorization
            .verification_url
            .starts_with("https://accounts.x.ai/"));
        assert!(!format!("{authorization:?}").contains("device-secret"));
        let tokens = poll_device_token(
            &client,
            &token_url,
            PUBLIC_CLIENT_ID,
            &authorization.device_code,
            Duration::from_millis(10),
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(tokens.access_token, "access-1");
        let refreshed = refresh_tokens_at(&client, &token_url, &tokens)
            .await
            .unwrap();
        assert_eq!(refreshed.access_token, "access-2");
        assert_eq!(refreshed.refresh_token, "refresh-1");
    }
}
