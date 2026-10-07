use base64::Engine as _;
use ring::{aead, agreement, digest, rand::SecureRandom as _};
use std::fmt::Write as _;

const PLATFORM_URL: &str = "https://platform.xiaomimimo.com";
const AUTHORIZE_PATH: &str = "/authorize";
#[cfg(any(not(target_os = "macos"), test))]
const MANUAL_REDIRECT: &str = "https://platform.xiaomimimo.com/authorize/code/callback";
const DEFAULT_API_URL: &str = "https://api.xiaomimimo.com/v1";
const X25519_SPKI_PREFIX: &[u8] = &[
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x6e, 0x03, 0x21, 0x00,
];
const MAX_RESULT_CHARS: usize = 96 * 1024;
#[cfg(any(target_os = "macos", test))]
const MAX_CALLBACK_BYTES: usize = 128 * 1024;
#[cfg(any(target_os = "macos", test))]
const MAX_INVALID_CALLBACKS: usize = 16;
#[cfg(target_os = "macos")]
const CALLBACK_TIMEOUT_SECS: u64 = 300;
#[cfg(any(target_os = "macos", test))]
const CALLBACK_HEADER_TIMEOUT_SECS: u64 = 2;
#[cfg(any(target_os = "macos", test))]
const CALLBACK_SUCCESS_REDIRECT: &str =
    "https://platform.xiaomimimo.com/authorize/callback?status=success";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MimoAuthError {
    #[error("MiMo authorization could not start")]
    Start,
    #[error("MiMo authorization result was invalid")]
    InvalidResult,
    #[error("MiMo credential and endpoint were inconsistent")]
    InconsistentCredentialEndpoint,
    #[cfg(any(target_os = "macos", test))]
    #[error("MiMo authorization callback was invalid: {0}")]
    Callback(&'static str),
    #[cfg(target_os = "macos")]
    #[error("MiMo authorization timed out")]
    TimedOut,
    #[cfg(target_os = "macos")]
    #[error("MiMo browser could not be opened")]
    Browser,
}

#[derive(Clone, PartialEq, Eq)]
pub struct MimoCredentials {
    pub api_key: String,
    pub base_url: String,
    pub uid: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MimoCredentialKind {
    Payg,
    TokenPlan(crate::pricing::MimoRegion),
}

impl MimoCredentials {
    pub fn kind(&self) -> Result<MimoCredentialKind, MimoAuthError> {
        classify_credential(self.api_key.as_bytes(), &self.base_url)
    }
}

pub fn classify_credential(
    api_key: &[u8],
    base_url: &str,
) -> Result<MimoCredentialKind, MimoAuthError> {
    let region = crate::pricing::MimoRegion::from_token_plan_url(base_url);
    if api_key.starts_with(b"tp-") {
        if !crate::pricing::valid_token_plan_key(api_key) {
            return Err(MimoAuthError::InvalidResult);
        }
        return region
            .map(MimoCredentialKind::TokenPlan)
            .ok_or(MimoAuthError::InconsistentCredentialEndpoint);
    }
    if region.is_some() {
        return Err(MimoAuthError::InconsistentCredentialEndpoint);
    }
    if api_key.is_empty()
        || api_key.len() > 64 * 1024
        || !api_key
            .iter()
            .all(|byte| byte.is_ascii_graphic() && !byte.is_ascii_whitespace())
    {
        return Err(MimoAuthError::InvalidResult);
    }
    Ok(MimoCredentialKind::Payg)
}

impl std::fmt::Debug for MimoCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MimoCredentials")
            .field("api_key", &"[REDACTED]")
            .field("base_url", &self.base_url)
            .field("uid", &"[REDACTED]")
            .finish()
    }
}

pub struct MimoAuthorization {
    authorization_url: String,
    private_key: Option<agreement::EphemeralPrivateKey>,
}

impl std::fmt::Debug for MimoAuthorization {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MimoAuthorization")
            .field("authorization_url", &self.authorization_url)
            .field("private_key", &"[REDACTED]")
            .finish()
    }
}

impl MimoAuthorization {
    #[cfg(any(not(target_os = "macos"), test))]
    pub fn manual() -> Result<Self, MimoAuthError> {
        Self::new(MANUAL_REDIRECT)
    }

    #[cfg(any(target_os = "macos", test))]
    pub fn loopback(port: u16) -> Result<Self, MimoAuthError> {
        Self::new(&format!("http://localhost:{port}/"))
    }

    fn new(redirect_uri: &str) -> Result<Self, MimoAuthError> {
        let rng = ring::rand::SystemRandom::new();
        let private_key = agreement::EphemeralPrivateKey::generate(&agreement::X25519, &rng)
            .map_err(|_| MimoAuthError::Start)?;
        let public_key = private_key
            .compute_public_key()
            .map_err(|_| MimoAuthError::Start)?;
        let mut spki = Vec::with_capacity(X25519_SPKI_PREFIX.len() + public_key.as_ref().len());
        spki.extend_from_slice(X25519_SPKI_PREFIX);
        spki.extend_from_slice(public_key.as_ref());
        let encoded_key = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(spki);
        let mut random = [0_u8; 4];
        rng.fill(&mut random).map_err(|_| MimoAuthError::Start)?;
        let mut key_name = String::with_capacity("mimo-code-cli-key-".len() + random.len() * 2);
        key_name.push_str("mimo-code-cli-key-");
        for byte in random {
            write!(&mut key_name, "{byte:02x}").map_err(|_| MimoAuthError::Start)?;
        }
        let mut url = reqwest::Url::parse(&format!("{PLATFORM_URL}{AUTHORIZE_PATH}"))
            .map_err(|_| MimoAuthError::Start)?;
        url.query_pairs_mut()
            .append_pair("pk", &encoded_key)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("kn", "mimocode")
            .append_pair("key_name", &key_name);
        Ok(Self {
            authorization_url: url.into(),
            private_key: Some(private_key),
        })
    }

    pub fn authorization_url(&self) -> &str {
        &self.authorization_url
    }

    pub fn decrypt_result(mut self, encoded: &str) -> Result<MimoCredentials, MimoAuthError> {
        let encoded = encoded.trim();
        if encoded.is_empty() || encoded.len() > MAX_RESULT_CHARS {
            return Err(MimoAuthError::InvalidResult);
        }
        let mut envelope = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| MimoAuthError::InvalidResult)?;
        if envelope.len() < 32 + 12 + aead::MAX_TAG_LEN {
            envelope.fill(0);
            return Err(MimoAuthError::InvalidResult);
        }
        let peer_key = envelope[..32].to_vec();
        let nonce_bytes: [u8; 12] = envelope[32..44]
            .try_into()
            .map_err(|_| MimoAuthError::InvalidResult)?;
        let mut ciphertext = envelope.split_off(44);
        envelope.fill(0);
        let private_key = self
            .private_key
            .take()
            .ok_or(MimoAuthError::InvalidResult)?;
        let peer_key = agreement::UnparsedPublicKey::new(&agreement::X25519, peer_key);
        let key_bytes = agreement::agree_ephemeral(private_key, &peer_key, |shared| {
            digest::digest(&digest::SHA256, shared).as_ref().to_vec()
        })
        .map_err(|_| MimoAuthError::InvalidResult)?;
        let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, &key_bytes)
            .map_err(|_| MimoAuthError::InvalidResult)?;
        let key = aead::LessSafeKey::new(unbound);
        let plaintext = key
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce_bytes),
                aead::Aad::empty(),
                &mut ciphertext,
            )
            .map_err(|_| MimoAuthError::InvalidResult)?;
        let result: DecryptedResult =
            serde_json::from_slice(plaintext).map_err(|_| MimoAuthError::InvalidResult)?;
        ciphertext.fill(0);
        let api_key = result
            .sk
            .filter(|value| !value.trim().is_empty())
            .ok_or(MimoAuthError::InvalidResult)?;
        if api_key.len() > 64 * 1024 || result.uid.len() > 4096 {
            return Err(MimoAuthError::InvalidResult);
        }
        let base_url = validate_api_url(result.url.as_deref().unwrap_or(DEFAULT_API_URL))?;
        Ok(MimoCredentials {
            api_key,
            base_url,
            uid: result.uid,
        })
    }
}

#[derive(serde::Deserialize)]
struct DecryptedResult {
    sk: Option<String>,
    #[serde(default)]
    uid: String,
    #[serde(default)]
    url: Option<String>,
}

fn validate_api_url(value: &str) -> Result<String, MimoAuthError> {
    let mut url = reqwest::Url::parse(value).map_err(|_| MimoAuthError::InvalidResult)?;
    let host = url.host_str().ok_or(MimoAuthError::InvalidResult)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || !(host == "xiaomimimo.com" || host.ends_with(".xiaomimimo.com"))
    {
        return Err(MimoAuthError::InvalidResult);
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.as_str().trim_end_matches('/').to_string())
}

#[cfg(any(target_os = "macos", test))]
fn parse_callback_request(bytes: &[u8], expected_host: &str) -> Result<String, MimoAuthError> {
    if bytes.len() > MAX_CALLBACK_BYTES {
        return Err(MimoAuthError::Callback("request was too large"));
    }
    let header_end = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(MimoAuthError::Callback("request headers were incomplete"))?;
    let headers = std::str::from_utf8(&bytes[..header_end + 4])
        .map_err(|_| MimoAuthError::Callback("request was not UTF-8"))?;
    let mut lines = headers.split("\r\n");
    let request_line = lines
        .next()
        .ok_or(MimoAuthError::Callback("request was empty"))?;
    let mut request = request_line.split_whitespace();
    if request.next() != Some("GET") {
        return Err(MimoAuthError::Callback("request method was not GET"));
    }
    let target = request
        .next()
        .ok_or(MimoAuthError::Callback("request target was missing"))?;
    if request.next() != Some("HTTP/1.1")
        || request.next().is_some()
        || !(target.starts_with("/?") || target.starts_with("/auth?"))
    {
        return Err(MimoAuthError::Callback("request path was unexpected"));
    }
    let host = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.trim());
    if host != Some(expected_host) {
        return Err(MimoAuthError::Callback("request host did not match"));
    }
    let query = target
        .strip_prefix("/auth?")
        .or_else(|| target.strip_prefix("/?"))
        .unwrap_or_default();
    let result = url::form_urlencoded::parse(query.as_bytes())
        .find(|(name, _)| name == "u")
        .map(|(_, value)| value.into_owned())
        .ok_or(MimoAuthError::Callback("authorization result was missing"))?;
    if result.is_empty() || result.len() > MAX_RESULT_CHARS {
        return Err(MimoAuthError::Callback(
            "authorization result had an invalid size",
        ));
    }
    Ok(result)
}

#[cfg(any(target_os = "macos", test))]
async fn read_callback_headers(
    stream: &mut tokio::net::TcpStream,
    header_timeout: std::time::Duration,
) -> Result<Vec<u8>, MimoAuthError> {
    use tokio::io::AsyncReadExt as _;

    let read = async {
        let mut bytes = Vec::new();
        loop {
            let mut chunk = [0_u8; 2048];
            let count = stream
                .read(&mut chunk)
                .await
                .map_err(|_| MimoAuthError::Callback("request could not be read"))?;
            if count == 0 {
                return Err(MimoAuthError::Callback("request headers were incomplete"));
            }
            if bytes.len().saturating_add(count) > MAX_CALLBACK_BYTES {
                return Err(MimoAuthError::Callback("request was too large"));
            }
            bytes.extend_from_slice(&chunk[..count]);
            if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                return Ok(bytes);
            }
        }
    };
    tokio::time::timeout(header_timeout, read)
        .await
        .map_err(|_| MimoAuthError::Callback("request headers timed out"))?
}

#[cfg(any(target_os = "macos", test))]
async fn write_callback_response(stream: &mut tokio::net::TcpStream, success: bool) {
    use tokio::io::AsyncWriteExt as _;

    let response = if success {
        format!(
            "HTTP/1.1 302 Found\r\nLocation: {CALLBACK_SUCCESS_REDIRECT}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 22\r\nConnection: close\r\nCache-Control: no-store\r\n\r\nAuthorization complete"
        )
    } else {
        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 30\r\nConnection: close\r\nCache-Control: no-store\r\n\r\nAuthorization callback invalid"
            .to_string()
    };
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

#[cfg(any(target_os = "macos", test))]
async fn receive_callback(
    listener: tokio::net::TcpListener,
    expected_host: String,
    authorization: MimoAuthorization,
    header_timeout: std::time::Duration,
) -> Result<MimoCredentials, MimoAuthError> {
    let mut invalid_connections = 0_usize;
    loop {
        let (mut stream, peer) = listener
            .accept()
            .await
            .map_err(|_| MimoAuthError::Callback("callback could not be accepted"))?;
        if !peer.ip().is_loopback() {
            write_callback_response(&mut stream, false).await;
            invalid_connections += 1;
        } else {
            let parsed = match read_callback_headers(&mut stream, header_timeout).await {
                Ok(bytes) => parse_callback_request(&bytes, &expected_host),
                Err(error) => Err(error),
            };
            match parsed {
                Ok(result) => match authorization.decrypt_result(&result) {
                    Ok(credentials) => {
                        write_callback_response(&mut stream, true).await;
                        return Ok(credentials);
                    }
                    Err(error) => {
                        write_callback_response(&mut stream, false).await;
                        return Err(error);
                    }
                },
                Err(_) => {
                    write_callback_response(&mut stream, false).await;
                    invalid_connections += 1;
                }
            }
        }
        if invalid_connections > MAX_INVALID_CALLBACKS {
            return Err(MimoAuthError::Callback(
                "too many invalid callback connections",
            ));
        }
    }
}

#[cfg(target_os = "macos")]
pub async fn authorize_browser_with_progress(
    progress: impl FnOnce(String) + Send,
) -> Result<MimoCredentials, MimoAuthError> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| MimoAuthError::Start)?;
    let port = listener
        .local_addr()
        .map_err(|_| MimoAuthError::Start)?
        .port();
    let authorization = MimoAuthorization::loopback(port)?;
    let url = authorization.authorization_url().to_string();
    progress(url.clone());
    let expected_host = format!("localhost:{port}");
    tokio::task::spawn_blocking(move || open_browser(&url))
        .await
        .map_err(|_| MimoAuthError::Browser)??;
    tokio::time::timeout(
        std::time::Duration::from_secs(CALLBACK_TIMEOUT_SECS),
        receive_callback(
            listener,
            expected_host,
            authorization,
            std::time::Duration::from_secs(CALLBACK_HEADER_TIMEOUT_SECS),
        ),
    )
    .await
    .map_err(|_| MimoAuthError::TimedOut)?
}

#[cfg(target_os = "macos")]
fn open_browser(url: &str) -> Result<(), MimoAuthError> {
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("/usr/bin/open");
    #[cfg(target_os = "linux")]
    let mut command = std::process::Command::new("xdg-open");
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let status = command
            .arg(url)
            .status()
            .map_err(|_| MimoAuthError::Browser)?;
        status.success().then_some(()).ok_or(MimoAuthError::Browser)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = url;
        Err(MimoAuthError::Browser)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    fn fixture_envelope(authorization: &MimoAuthorization, value: serde_json::Value) -> String {
        let url = reqwest::Url::parse(authorization.authorization_url()).unwrap();
        let encoded_public = url
            .query_pairs()
            .find(|(name, _)| name == "pk")
            .unwrap()
            .1
            .into_owned();
        let spki = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded_public)
            .unwrap();
        assert_eq!(&spki[..X25519_SPKI_PREFIX.len()], X25519_SPKI_PREFIX);
        let client_public = &spki[X25519_SPKI_PREFIX.len()..];
        let rng = ring::rand::SystemRandom::new();
        let server_private =
            agreement::EphemeralPrivateKey::generate(&agreement::X25519, &rng).unwrap();
        let server_public = server_private.compute_public_key().unwrap();
        let peer = agreement::UnparsedPublicKey::new(&agreement::X25519, client_public);
        let key_bytes = agreement::agree_ephemeral(server_private, &peer, |shared| {
            digest::digest(&digest::SHA256, shared).as_ref().to_vec()
        })
        .unwrap();
        let key =
            aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM, &key_bytes).unwrap());
        let nonce_bytes = [7_u8; 12];
        let mut ciphertext = serde_json::to_vec(&value).unwrap();
        key.seal_in_place_append_tag(
            aead::Nonce::assume_unique_for_key(nonce_bytes),
            aead::Aad::empty(),
            &mut ciphertext,
        )
        .unwrap();
        let mut envelope = server_public.as_ref().to_vec();
        envelope.extend_from_slice(&nonce_bytes);
        envelope.extend_from_slice(&ciphertext);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(envelope)
    }

    #[test]
    fn manual_url_uses_published_contract_and_ephemeral_spki() {
        let authorization = MimoAuthorization::manual().unwrap();
        let url = reqwest::Url::parse(authorization.authorization_url()).unwrap();
        let params = url
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(
            url.as_str().split('?').next().unwrap(),
            format!("{PLATFORM_URL}/authorize")
        );
        assert_eq!(params.get("redirect_uri").unwrap(), MANUAL_REDIRECT);
        assert_eq!(params.get("kn").unwrap(), "mimocode");
        let key_name = params.get("key_name").unwrap();
        let suffix = key_name.strip_prefix("mimo-code-cli-key-").unwrap();
        assert_eq!(suffix.len(), 8);
        assert!(suffix.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(suffix, suffix.to_ascii_lowercase());
        let spki = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(params.get("pk").unwrap().as_bytes())
            .unwrap();
        assert_eq!(spki.len(), X25519_SPKI_PREFIX.len() + 32);
        assert_eq!(&spki[..X25519_SPKI_PREFIX.len()], X25519_SPKI_PREFIX);

        let loopback = MimoAuthorization::loopback(3210).unwrap();
        let loopback_url = reqwest::Url::parse(loopback.authorization_url()).unwrap();
        assert!(loopback_url
            .query_pairs()
            .any(|(name, value)| name == "redirect_uri" && value == "http://localhost:3210/"));
    }

    #[test]
    fn decrypts_published_x25519_aes_gcm_envelope() {
        let authorization = MimoAuthorization::manual().unwrap();
        let envelope = fixture_envelope(
            &authorization,
            serde_json::json!({
                "sk": "fixture-secret",
                "uid": "fixture-user",
                "url": "https://token-plan-sgp.xiaomimimo.com/v1"
            }),
        );
        let credentials = authorization.decrypt_result(&envelope).unwrap();
        assert_eq!(credentials.api_key, "fixture-secret");
        assert_eq!(credentials.uid, "fixture-user");
        assert_eq!(
            credentials.base_url,
            "https://token-plan-sgp.xiaomimimo.com/v1"
        );
        assert!(!format!("{credentials:?}").contains("fixture-secret"));
    }

    #[test]
    fn rejects_malformed_oversized_or_untrusted_results() {
        assert_eq!(
            MimoAuthorization::manual()
                .unwrap()
                .decrypt_result("not-base64"),
            Err(MimoAuthError::InvalidResult)
        );
        assert_eq!(
            MimoAuthorization::manual()
                .unwrap()
                .decrypt_result(&"x".repeat(MAX_RESULT_CHARS + 1)),
            Err(MimoAuthError::InvalidResult)
        );
        let authorization = MimoAuthorization::manual().unwrap();
        let envelope = fixture_envelope(
            &authorization,
            serde_json::json!({
                "sk": "fixture-secret",
                "uid": "fixture-user",
                "url": "https://attacker.example/v1"
            }),
        );
        assert_eq!(
            authorization.decrypt_result(&envelope),
            Err(MimoAuthError::InvalidResult)
        );
    }

    #[test]
    fn classifies_payg_and_all_exact_token_plan_regions_without_secret_disclosure() {
        assert_eq!(
            classify_credential(b"sk-payg", DEFAULT_API_URL),
            Ok(MimoCredentialKind::Payg)
        );
        for region in crate::pricing::MimoRegion::ALL {
            assert_eq!(
                classify_credential(b"tp-fixture", region.token_plan_url()),
                Ok(MimoCredentialKind::TokenPlan(region))
            );
            assert_eq!(
                classify_credential(b"tp-fixture", &format!("{}/", region.token_plan_url())),
                Ok(MimoCredentialKind::TokenPlan(region))
            );
        }

        let token_on_payg = classify_credential(b"tp-never-render", DEFAULT_API_URL).unwrap_err();
        let payg_on_token = classify_credential(
            b"sk-never-render",
            crate::pricing::MimoRegion::Singapore.token_plan_url(),
        )
        .unwrap_err();
        assert_eq!(token_on_payg, MimoAuthError::InconsistentCredentialEndpoint);
        assert_eq!(payg_on_token, MimoAuthError::InconsistentCredentialEndpoint);
        let rendered = format!("{token_on_payg}; {payg_on_token}");
        assert!(!rendered.contains("never-render"));
    }

    #[test]
    fn validates_loopback_callback_host_path_and_size() {
        let result = parse_callback_request(
            b"GET /auth?u=encrypted-result HTTP/1.1\r\nHost: localhost:3210\r\n\r\n",
            "localhost:3210",
        )
        .unwrap();
        assert_eq!(result, "encrypted-result");
        assert_eq!(
            parse_callback_request(
                b"GET /?u=legacy-result HTTP/1.1\r\nHost: localhost:3210\r\n\r\n",
                "localhost:3210",
            )
            .unwrap(),
            "legacy-result"
        );
        assert!(parse_callback_request(
            b"GET /?u=x HTTP/1.1\r\nHost: attacker.example\r\n\r\n",
            "localhost:3210"
        )
        .is_err());
        assert!(parse_callback_request(
            b"GET /wrong?u=x HTTP/1.1\r\nHost: localhost:3210\r\n\r\n",
            "localhost:3210"
        )
        .is_err());
        assert!(
            parse_callback_request(&vec![b'x'; MAX_CALLBACK_BYTES + 1], "localhost:3210").is_err()
        );
        assert!(parse_callback_request(
            b"GET /?u=x HTTP/1.1\r\nHost: localhost:3210\r\n",
            "localhost:3210"
        )
        .is_err());
    }

    async fn callback_fixture() -> (
        tokio::net::TcpListener,
        std::net::SocketAddr,
        String,
        MimoAuthorization,
    ) {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let host = format!("localhost:{}", address.port());
        let authorization = MimoAuthorization::loopback(address.port()).unwrap();
        (listener, address, host, authorization)
    }

    async fn send_request(address: std::net::SocketAddr, request: &[u8]) -> String {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream.write_all(request).await.unwrap();
        stream.shutdown().await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn speculative_empty_connection_is_ignored_before_valid_callback() {
        let (listener, address, host, authorization) = callback_fixture().await;
        let envelope = fixture_envelope(
            &authorization,
            serde_json::json!({
                "sk": "fixture-secret",
                "uid": "fixture-user",
                "url": DEFAULT_API_URL
            }),
        );
        let receiver = tokio::spawn(receive_callback(
            listener,
            host.clone(),
            authorization,
            std::time::Duration::from_millis(50),
        ));
        drop(tokio::net::TcpStream::connect(address).await.unwrap());
        let request = format!("GET /?u={envelope} HTTP/1.1\r\nHost: {host}\r\n\r\n");
        let response = send_request(address, request.as_bytes()).await;
        let credentials = receiver.await.unwrap().unwrap();

        assert_eq!(credentials.api_key, "fixture-secret");
        assert!(response.starts_with("HTTP/1.1 302 Found\r\n"));
        assert!(response.contains(&format!("Location: {CALLBACK_SUCCESS_REDIRECT}\r\n")));
        assert!(!response.contains("fixture-secret"));
    }

    #[tokio::test]
    async fn partial_timeout_bad_host_path_and_oversize_do_not_consume_callback() {
        let (listener, address, host, authorization) = callback_fixture().await;
        let envelope = fixture_envelope(
            &authorization,
            serde_json::json!({"sk": "fixture-secret", "uid": "fixture-user"}),
        );
        let receiver = tokio::spawn(receive_callback(
            listener,
            host.clone(),
            authorization,
            std::time::Duration::from_millis(20),
        ));

        let mut partial = tokio::net::TcpStream::connect(address).await.unwrap();
        partial.write_all(b"GET /?u=x HTTP/1.1\r\n").await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        drop(partial);
        let _ = send_request(
            address,
            b"GET /wrong?u=x HTTP/1.1\r\nHost: localhost:1\r\n\r\n",
        )
        .await;
        let bad_host = "GET /?u=x HTTP/1.1\r\nHost: localhost:1\r\n\r\n".to_string();
        let _ = send_request(address, bad_host.as_bytes()).await;
        let mut oversized = tokio::net::TcpStream::connect(address).await.unwrap();
        let _ = oversized
            .write_all(&vec![b'x'; MAX_CALLBACK_BYTES + 1])
            .await;
        drop(oversized);

        let request = format!("GET /auth?u={envelope} HTTP/1.1\r\nHost: {host}\r\n\r\n");
        let response = send_request(address, request.as_bytes()).await;
        let credentials = receiver.await.unwrap().unwrap();
        assert_eq!(credentials.api_key, "fixture-secret");
        assert!(response.contains("302 Found"));
    }

    #[tokio::test]
    async fn structurally_valid_invalid_ciphertext_fails_without_success_redirect() {
        let (listener, address, host, authorization) = callback_fixture().await;
        let receiver = tokio::spawn(receive_callback(
            listener,
            host.clone(),
            authorization,
            std::time::Duration::from_millis(50),
        ));
        let request = format!("GET /?u=not-a-valid-envelope HTTP/1.1\r\nHost: {host}\r\n\r\n");
        let response = send_request(address, request.as_bytes()).await;
        assert_eq!(receiver.await.unwrap(), Err(MimoAuthError::InvalidResult));
        assert!(response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
        assert!(!response.contains(CALLBACK_SUCCESS_REDIRECT));
        assert!(!response.contains("not-a-valid-envelope"));
    }

    #[tokio::test]
    async fn overall_callback_deadline_remains_external_and_bounded() {
        let (listener, _address, host, authorization) = callback_fixture().await;
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(20),
            receive_callback(
                listener,
                host,
                authorization,
                std::time::Duration::from_secs(CALLBACK_HEADER_TIMEOUT_SECS),
            ),
        )
        .await;
        assert!(result.is_err());
    }
}
