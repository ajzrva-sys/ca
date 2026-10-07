use crate::config::{EmailGatewayConfig, EmailTlsEndpoint, EmailTlsMode};
use async_trait::async_trait;
use base64::Engine as _;
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::TlsConnector;

const IO_TIMEOUT_SECS: u64 = 30;
const SMTP_RESPONSE_MAX: usize = 64 * 1024;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ImapError {
    #[error("email gateway transport is unavailable")]
    Unavailable,
    #[error("email gateway transport returned invalid bounded data")]
    Invalid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MailboxStatus {
    pub uid_validity: u32,
    pub uid_next: u32,
    pub highest_uid: u32,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RawInbound {
    pub uid: u32,
    pub raw: Vec<u8>,
}

impl std::fmt::Debug for RawInbound {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RawInbound")
            .field("uid", &self.uid)
            .field("raw", &"[PRIVATE RFC5322 BYTES]")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendOutcome {
    Appended { drafts_uid: u32 },
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchOutcome {
    Unique { drafts_uid: u32 },
    AbsentTrustworthy,
    Multiple,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConnectDoctorStatus {
    pub imap_tls_authenticated: bool,
    pub smtp_tls_authenticated: bool,
}

#[async_trait]
pub trait GatewayTransport: Send + Sync {
    async fn connect_doctor(&self, secret: &[u8]) -> Result<ConnectDoctorStatus, ImapError>;
    async fn inbox_status(&self, secret: &[u8]) -> Result<MailboxStatus, ImapError>;
    async fn fetch_after(
        &self,
        secret: &[u8],
        after_uid: u32,
        limit: usize,
        max_raw_bytes: usize,
    ) -> Result<Vec<RawInbound>, ImapError>;
    async fn append_draft(
        &self,
        secret: &[u8],
        bytes: &[u8],
        message_id: &str,
    ) -> Result<AppendOutcome, ImapError>;
    async fn search_draft(
        &self,
        secret: &[u8],
        message_id: &str,
    ) -> Result<SearchOutcome, ImapError>;
}

#[derive(Clone)]
pub struct RustlsImapTransport {
    config: EmailGatewayConfig,
}

impl RustlsImapTransport {
    pub fn new(config: &EmailGatewayConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}

impl std::fmt::Debug for RustlsImapTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RustlsImapTransport")
            .field("account", &"[PRIVATE]")
            .field("imap", &self.config.imap)
            .field("smtp", &self.config.smtp)
            .finish()
    }
}

type ImapSession = async_imap::Session<TlsStream<TcpStream>>;

#[async_trait]
impl GatewayTransport for RustlsImapTransport {
    async fn connect_doctor(&self, secret: &[u8]) -> Result<ConnectDoctorStatus, ImapError> {
        let mut session = timed(connect_imap(&self.config, secret)).await?;
        timed(session.logout()).await?;
        timed(smtp_auth_check(&self.config, secret)).await?;
        Ok(ConnectDoctorStatus {
            imap_tls_authenticated: true,
            smtp_tls_authenticated: true,
        })
    }

    async fn inbox_status(&self, secret: &[u8]) -> Result<MailboxStatus, ImapError> {
        let mut session = timed(connect_imap(&self.config, secret)).await?;
        let mailbox = timed(session.examine(&self.config.inbox_folder)).await?;
        let status = MailboxStatus {
            uid_validity: mailbox.uid_validity.ok_or(ImapError::Invalid)?,
            uid_next: mailbox.uid_next.ok_or(ImapError::Invalid)?,
            highest_uid: mailbox.uid_next.unwrap_or(1).saturating_sub(1),
        };
        let _ = timed(session.logout()).await;
        Ok(status)
    }

    async fn fetch_after(
        &self,
        secret: &[u8],
        after_uid: u32,
        limit: usize,
        max_raw_bytes: usize,
    ) -> Result<Vec<RawInbound>, ImapError> {
        if limit == 0 || limit > 64 {
            return Err(ImapError::Invalid);
        }
        let mut session = timed(connect_imap(&self.config, secret)).await?;
        timed(session.examine(&self.config.inbox_folder)).await?;
        let range = format!("{}:*", after_uid.saturating_add(1));
        let stream = timed(session.uid_fetch(range, "(UID RFC822.SIZE BODY.PEEK[])")).await?;
        let rows = timed(stream.try_collect::<Vec<_>>()).await?;
        let mut output = Vec::new();
        for row in rows {
            let uid = row.uid.ok_or(ImapError::Invalid)?;
            if uid <= after_uid {
                continue;
            }
            if row.size.is_some_and(|size| size as usize > max_raw_bytes) {
                return Err(ImapError::Invalid);
            }
            let raw = row.body().ok_or(ImapError::Invalid)?;
            if raw.len() > max_raw_bytes {
                return Err(ImapError::Invalid);
            }
            output.push(RawInbound {
                uid,
                raw: raw.to_vec(),
            });
        }
        output.sort_by_key(|row| row.uid);
        if output.len() > limit {
            output.truncate(limit);
        }
        let _ = timed(session.logout()).await;
        Ok(output)
    }

    async fn append_draft(
        &self,
        secret: &[u8],
        bytes: &[u8],
        message_id: &str,
    ) -> Result<AppendOutcome, ImapError> {
        if bytes.is_empty()
            || bytes.len() > self.config.max_response_bytes.saturating_add(16 * 1024)
        {
            return Err(ImapError::Invalid);
        }
        let mut session = timed(connect_imap(&self.config, secret)).await?;
        if timed(session.append(&self.config.drafts_folder, Some("(\\Draft)"), None, bytes))
            .await
            .is_err()
        {
            return Ok(AppendOutcome::Ambiguous);
        }
        drop(session);
        match self.search_draft(secret, message_id).await? {
            SearchOutcome::Unique { drafts_uid } => Ok(AppendOutcome::Appended { drafts_uid }),
            _ => Ok(AppendOutcome::Ambiguous),
        }
    }

    async fn search_draft(
        &self,
        secret: &[u8],
        message_id: &str,
    ) -> Result<SearchOutcome, ImapError> {
        if !safe_message_id(message_id) {
            return Err(ImapError::Invalid);
        }
        let mut session = match timed(connect_imap(&self.config, secret)).await {
            Ok(session) => session,
            Err(_) => return Ok(SearchOutcome::Unavailable),
        };
        if timed(session.examine(&self.config.drafts_folder))
            .await
            .is_err()
        {
            return Ok(SearchOutcome::Unavailable);
        }
        let query = format!("HEADER Message-ID \"{message_id}\"");
        let matches = match timed(session.uid_search(query)).await {
            Ok(matches) => matches,
            Err(_) => return Ok(SearchOutcome::Unavailable),
        };
        let _ = timed(session.logout()).await;
        match matches.len() {
            0 => Ok(SearchOutcome::AbsentTrustworthy),
            1 => Ok(SearchOutcome::Unique {
                drafts_uid: *matches.iter().next().expect("one result"),
            }),
            _ => Ok(SearchOutcome::Multiple),
        }
    }
}

async fn connect_imap(
    config: &EmailGatewayConfig,
    secret: &[u8],
) -> Result<ImapSession, ImapError> {
    let password = std::str::from_utf8(secret).map_err(|_| ImapError::Invalid)?;
    let (tls, needs_greeting) = connect_tls(&config.imap).await?;
    let mut client = async_imap::Client::new(tls);
    if needs_greeting {
        client
            .read_response()
            .await
            .map_err(|_| ImapError::Unavailable)?
            .ok_or(ImapError::Unavailable)?;
    }
    client
        .login(&config.address, password)
        .await
        .map_err(|_| ImapError::Unavailable)
}

async fn connect_tls(
    endpoint: &EmailTlsEndpoint,
) -> Result<(TlsStream<TcpStream>, bool), ImapError> {
    use rustls_platform_verifier::ConfigVerifierExt;
    let tcp = TcpStream::connect((endpoint.host.as_str(), endpoint.port))
        .await
        .map_err(|_| ImapError::Unavailable)?;
    let connector = TlsConnector::from(Arc::new(
        tokio_rustls::rustls::ClientConfig::with_platform_verifier()
            .map_err(|_| ImapError::Unavailable)?,
    ));
    let server_name =
        ServerName::try_from(endpoint.host.clone()).map_err(|_| ImapError::Invalid)?;
    match endpoint.mode {
        EmailTlsMode::Imaps => connector
            .connect(server_name, tcp)
            .await
            .map(|stream| (stream, true))
            .map_err(|_| ImapError::Unavailable),
        EmailTlsMode::StartTlsRequired => {
            let mut client = async_imap::Client::new(tcp);
            client
                .read_response()
                .await
                .map_err(|_| ImapError::Unavailable)?
                .ok_or(ImapError::Unavailable)?;
            client
                .run_command_and_check_ok("STARTTLS", None)
                .await
                .map_err(|_| ImapError::Unavailable)?;
            connector
                .connect(server_name, client.into_inner())
                .await
                .map(|stream| (stream, false))
                .map_err(|_| ImapError::Unavailable)
        }
    }
}

async fn smtp_auth_check(config: &EmailGatewayConfig, secret: &[u8]) -> Result<(), ImapError> {
    let password = std::str::from_utf8(secret).map_err(|_| ImapError::Invalid)?;
    let (mut stream, needs_greeting): (Box<dyn AsyncIo>, bool) = match config.smtp.mode {
        EmailTlsMode::Imaps => {
            let (stream, needs_greeting) = connect_tls(&config.smtp).await?;
            (Box::new(stream), needs_greeting)
        }
        EmailTlsMode::StartTlsRequired => {
            let mut tcp = TcpStream::connect((config.smtp.host.as_str(), config.smtp.port))
                .await
                .map_err(|_| ImapError::Unavailable)?;
            expect_smtp(&mut tcp, 220).await?;
            send_smtp(&mut tcp, b"EHLO localhost\r\n", 250).await?;
            send_smtp(&mut tcp, b"STARTTLS\r\n", 220).await?;
            (
                Box::new(connect_tls_stream(&config.smtp.host, tcp).await?),
                false,
            )
        }
    };
    if needs_greeting {
        expect_smtp(&mut stream, 220).await?;
    }
    send_smtp(&mut stream, b"EHLO localhost\r\n", 250).await?;
    let auth = base64::engine::general_purpose::STANDARD
        .encode(format!("\0{}\0{}", config.address, password));
    send_smtp(
        &mut stream,
        format!("AUTH PLAIN {auth}\r\n").as_bytes(),
        235,
    )
    .await?;
    let _ = send_smtp(&mut stream, b"QUIT\r\n", 221).await;
    Ok(())
}

trait AsyncIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncIo for T {}

async fn connect_tls_stream(host: &str, tcp: TcpStream) -> Result<TlsStream<TcpStream>, ImapError> {
    use rustls_platform_verifier::ConfigVerifierExt;
    let connector = TlsConnector::from(Arc::new(
        tokio_rustls::rustls::ClientConfig::with_platform_verifier()
            .map_err(|_| ImapError::Unavailable)?,
    ));
    let name = ServerName::try_from(host.to_owned()).map_err(|_| ImapError::Invalid)?;
    connector
        .connect(name, tcp)
        .await
        .map_err(|_| ImapError::Unavailable)
}

async fn send_smtp<S: AsyncRead + AsyncWrite + Unpin + ?Sized>(
    stream: &mut S,
    bytes: &[u8],
    expected: u16,
) -> Result<(), ImapError> {
    stream
        .write_all(bytes)
        .await
        .map_err(|_| ImapError::Unavailable)?;
    stream.flush().await.map_err(|_| ImapError::Unavailable)?;
    expect_smtp(stream, expected).await
}

async fn expect_smtp<S: AsyncRead + Unpin + ?Sized>(
    stream: &mut S,
    expected: u16,
) -> Result<(), ImapError> {
    let mut total = 0usize;
    loop {
        let mut line = Vec::new();
        loop {
            let byte = stream.read_u8().await.map_err(|_| ImapError::Unavailable)?;
            total += 1;
            if total > SMTP_RESPONSE_MAX {
                return Err(ImapError::Invalid);
            }
            line.push(byte);
            if byte == b'\n' {
                break;
            }
        }
        if line.len() < 4 {
            return Err(ImapError::Invalid);
        }
        let code = std::str::from_utf8(&line[..3])
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .ok_or(ImapError::Invalid)?;
        if code != expected {
            return Err(ImapError::Unavailable);
        }
        if line[3] == b' ' {
            return Ok(());
        }
        if line[3] != b'-' {
            return Err(ImapError::Invalid);
        }
    }
}

async fn timed<F, T, E>(future: F) -> Result<T, ImapError>
where
    F: std::future::Future<Output = Result<T, E>>,
{
    tokio::time::timeout(std::time::Duration::from_secs(IO_TIMEOUT_SECS), future)
        .await
        .map_err(|_| ImapError::Unavailable)?
        .map_err(|_| ImapError::Unavailable)
}

fn safe_message_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.contains(['"', '\\', '\r', '\n'])
        && value.starts_with('<')
        && value.ends_with('>')
}
