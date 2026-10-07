use crate::config::{ProviderConfig, VoiceBackendConfig};
use futures::StreamExt;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(target_os = "macos")]
const MAX_AUDIO_BYTES: usize = 32 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
pub const MAX_TRANSCRIPT_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoicePhase {
    Ready,
    Recording,
    Transcribing,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceState {
    pub phase: VoicePhase,
    pub generation: u64,
    pub error: Option<String>,
}

impl VoiceState {
    pub fn ready(generation: u64) -> Self {
        Self {
            phase: VoicePhase::Ready,
            generation,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureControl {
    Stop,
    Cancel,
}

pub struct VoiceAudio {
    wav: Vec<u8>,
}

impl VoiceAudio {
    #[cfg(test)]
    fn test_wav(wav: Vec<u8>) -> Self {
        Self { wav }
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum VoiceError {
    #[error("Voice input is disabled; configure [voice] and restart")]
    Disabled,
    #[error("Voice input is not configured")]
    NotConfigured,
    #[error("Microphone input is unavailable")]
    CaptureUnavailable,
    #[error("Microphone recording failed")]
    CaptureFailed,
    #[error("No speech was recorded")]
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    EmptyAudio,
    #[error("Voice recording was cancelled")]
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Cancelled,
    #[error("The configured transcription provider is unavailable")]
    ProviderUnavailable,
    #[error("The transcription credential is missing")]
    MissingCredential,
    #[error("Authentication rejected by the transcription provider (HTTP 401)")]
    AuthenticationRejected,
    #[error("Access denied by the transcription provider (HTTP 403)")]
    AccessDenied,
    #[error("The transcription endpoint was not found (HTTP 404)")]
    EndpointNotFound,
    #[error("The transcription provider is rate limited (HTTP 429)")]
    RateLimited,
    #[error("The transcription provider returned HTTP {0}")]
    ProviderHttp(u16),
    #[error("Could not connect to the transcription provider")]
    CannotConnect,
    #[error("Transcription timed out")]
    TimedOut,
    #[error("The transcription response was invalid or incomplete")]
    InvalidResponse,
    #[error("The local transcriber cannot run in a network-denied sandbox")]
    SandboxUnavailable,
    #[error("The local transcriber could not be started")]
    LocalStartFailed,
    #[error("The local transcriber failed")]
    LocalFailed,
}

pub struct VoiceRunConfig {
    pub backend: VoiceBackendConfig,
    pub providers: HashMap<String, ProviderConfig>,
    pub max_seconds: u64,
    pub workspace_roots: Vec<PathBuf>,
    pub usage: Option<crate::usage::UsageRecorder>,
}

struct ProcessGroupGuard(Option<u32>);

impl ProcessGroupGuard {
    fn new(process_id: Option<u32>) -> Self {
        Self(process_id)
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        if let Some(id) = self.0 {
            #[cfg(unix)]
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(-(id as i32)),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

pub async fn run_voice_session(
    config: VoiceRunConfig,
    control: std::sync::mpsc::Receiver<CaptureControl>,
    on_transcribing: impl FnOnce() + Send + 'static,
) -> Result<String, VoiceError> {
    let max_seconds = config.max_seconds;
    let audio = tokio::task::spawn_blocking(move || capture_audio(max_seconds, control))
        .await
        .map_err(|_| VoiceError::CaptureFailed)??;
    on_transcribing();
    transcribe(
        &config.backend,
        &config.providers,
        audio,
        &config.workspace_roots,
        config.usage,
    )
    .await
}

#[cfg(target_os = "macos")]
fn capture_audio(
    max_seconds: u64,
    control: std::sync::mpsc::Receiver<CaptureControl>,
) -> Result<VoiceAudio, VoiceError> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::{Arc, Mutex};

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or(VoiceError::CaptureUnavailable)?;
    let supported = device
        .default_input_config()
        .map_err(|_| VoiceError::CaptureUnavailable)?;
    let sample_rate = supported.sample_rate().0;
    let channels = usize::from(supported.channels()).max(1);
    let maximum_samples = usize::try_from(u64::from(sample_rate) * max_seconds)
        .unwrap_or(usize::MAX)
        .min((MAX_AUDIO_BYTES - 44) / std::mem::size_of::<i16>());
    let samples = Arc::new(Mutex::new(Vec::<i16>::with_capacity(
        maximum_samples.min(256 * 1024),
    )));
    let callback_samples = samples.clone();
    let callback_error = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stream_error = callback_error.clone();
    let stream_config: cpal::StreamConfig = supported.clone().into();

    macro_rules! build_stream {
        ($sample:ty, $convert:expr) => {{
            let callback_samples = callback_samples.clone();
            device.build_input_stream(
                &stream_config,
                move |input: &[$sample], _| {
                    let Ok(mut output) = callback_samples.try_lock() else {
                        return;
                    };
                    if output.len() >= maximum_samples {
                        return;
                    }
                    for frame in input.chunks(channels) {
                        if output.len() >= maximum_samples {
                            break;
                        }
                        if let Some(sample) = frame.first() {
                            output.push(($convert)(*sample));
                        }
                    }
                },
                move |_| {
                    stream_error.store(true, std::sync::atomic::Ordering::Release);
                },
                None,
            )
        }};
    }

    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build_stream!(f32, |sample: f32| {
            (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
        }),
        cpal::SampleFormat::I16 => build_stream!(i16, |sample: i16| sample),
        cpal::SampleFormat::U16 => {
            build_stream!(u16, |sample: u16| { (i32::from(sample) - 32_768) as i16 })
        }
        _ => return Err(VoiceError::CaptureUnavailable),
    }
    .map_err(|_| VoiceError::CaptureUnavailable)?;
    stream.play().map_err(|_| VoiceError::CaptureFailed)?;

    let deadline = std::time::Instant::now() + Duration::from_secs(max_seconds);
    loop {
        if callback_error.load(std::sync::atomic::Ordering::Acquire) {
            return Err(VoiceError::CaptureFailed);
        }
        if samples.lock().map_err(|_| VoiceError::CaptureFailed)?.len() >= maximum_samples {
            break;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match control.recv_timeout(remaining.min(Duration::from_millis(50))) {
            Ok(CaptureControl::Stop) => break,
            Ok(CaptureControl::Cancel) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(VoiceError::Cancelled)
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    drop(stream);
    let samples = Arc::try_unwrap(samples)
        .map_err(|_| VoiceError::CaptureFailed)?
        .into_inner()
        .map_err(|_| VoiceError::CaptureFailed)?;
    encode_wav(samples, sample_rate)
}

#[cfg(not(target_os = "macos"))]
fn capture_audio(
    _max_seconds: u64,
    _control: std::sync::mpsc::Receiver<CaptureControl>,
) -> Result<VoiceAudio, VoiceError> {
    Err(VoiceError::CaptureUnavailable)
}

#[cfg(target_os = "macos")]
fn encode_wav(samples: Vec<i16>, sample_rate: u32) -> Result<VoiceAudio, VoiceError> {
    if samples.is_empty() {
        return Err(VoiceError::EmptyAudio);
    }
    let mut wav = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut wav);
        let mut writer = hound::WavWriter::new(
            cursor,
            hound::WavSpec {
                channels: 1,
                sample_rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .map_err(|_| VoiceError::CaptureFailed)?;
        for sample in samples {
            writer
                .write_sample(sample)
                .map_err(|_| VoiceError::CaptureFailed)?;
        }
        writer.finalize().map_err(|_| VoiceError::CaptureFailed)?;
    }
    if wav.len() > MAX_AUDIO_BYTES {
        return Err(VoiceError::CaptureFailed);
    }
    Ok(VoiceAudio { wav })
}

pub async fn transcribe(
    backend: &VoiceBackendConfig,
    providers: &HashMap<String, ProviderConfig>,
    audio: VoiceAudio,
    workspace_roots: &[PathBuf],
    usage: Option<crate::usage::UsageRecorder>,
) -> Result<String, VoiceError> {
    match backend {
        VoiceBackendConfig::Local {
            executable,
            args,
            timeout_secs,
        } => {
            transcribe_local(
                executable,
                args,
                Duration::from_secs(*timeout_secs),
                audio,
                workspace_roots,
            )
            .await
        }
        VoiceBackendConfig::Provider {
            provider,
            model,
            timeout_secs,
        } => {
            let provider = providers
                .get(provider)
                .ok_or(VoiceError::ProviderUnavailable)?;
            let span = usage.map(|recorder| recorder.begin());
            let result =
                transcribe_provider(provider, model, Duration::from_secs(*timeout_secs), audio)
                    .await;
            if let Some(span) = span {
                if result.is_ok() {
                    span.complete(None);
                } else {
                    span.fail(None);
                }
            }
            result
        }
    }
}

async fn transcribe_local(
    executable: &Path,
    args: &[String],
    timeout: Duration,
    audio: VoiceAudio,
    workspace_roots: &[PathBuf],
) -> Result<String, VoiceError> {
    use tokio::io::AsyncReadExt;

    let temporary = tempfile::Builder::new()
        .prefix("ca-voice-")
        .tempdir()
        .map_err(|_| VoiceError::LocalStartFailed)?;
    let audio_path = temporary.path().join("input.wav");
    std::fs::write(&audio_path, &audio.wav).map_err(|_| VoiceError::LocalStartFailed)?;
    let audio_value = audio_path.to_string_lossy();
    let arguments = args
        .iter()
        .map(|argument| argument.replace("{audio}", &audio_value))
        .collect::<Vec<_>>();
    let mut sandbox_roots = workspace_roots
        .iter()
        .map(|root| root.canonicalize().unwrap_or_else(|_| root.clone()))
        .collect::<Vec<_>>();
    let canonical_temporary = temporary
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temporary.path().to_path_buf());
    sandbox_roots.push(canonical_temporary.clone());
    let prepared = crate::sandbox::SandboxBackend::new(crate::sandbox::SandboxMode::Required)
        .prepare_process(
            executable,
            &arguments,
            &crate::sandbox::SandboxPolicy {
                roots: sandbox_roots,
                read_only_roots: Vec::new(),
                temporary_directory: canonical_temporary,
                network_allowed: false,
                hide_other_processes: false,
            },
        )
        .map_err(|_| VoiceError::SandboxUnavailable)?;
    let mut command = tokio::process::Command::new(prepared.program);
    command
        .args(prepared.arguments)
        .current_dir(temporary.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("TMPDIR", temporary.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    let mut child = command.spawn().map_err(|_| VoiceError::LocalStartFailed)?;
    let process_id = child.id();
    let process_group = ProcessGroupGuard::new(process_id);
    let mut stdout = child.stdout.take().ok_or(VoiceError::LocalStartFailed)?;
    let output = tokio::spawn(async move {
        let mut collected = Vec::new();
        let mut buffer = [0u8; 8 * 1024];
        let mut total = 0usize;
        loop {
            let read = stdout.read(&mut buffer).await.map_err(|_| ())?;
            if read == 0 {
                break;
            }
            total = total.saturating_add(read);
            if collected.len() < MAX_RESPONSE_BYTES {
                let retain = (MAX_RESPONSE_BYTES - collected.len()).min(read);
                collected.extend_from_slice(&buffer[..retain]);
            }
        }
        Ok::<_, ()>((collected, total))
    });
    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(_)) => return Err(VoiceError::LocalFailed),
        Err(_) => {
            drop(process_group);
            let _ = child.wait().await;
            output.abort();
            return Err(VoiceError::TimedOut);
        }
    };
    drop(process_group);
    let (bytes, total) = output
        .await
        .map_err(|_| VoiceError::LocalFailed)?
        .map_err(|_| VoiceError::LocalFailed)?;
    if !status.success() || total > MAX_RESPONSE_BYTES {
        return Err(VoiceError::LocalFailed);
    }
    transcript_from_bytes(bytes)
}

async fn transcribe_provider(
    provider: &ProviderConfig,
    model: &str,
    timeout: Duration,
    audio: VoiceAudio,
) -> Result<String, VoiceError> {
    let raw_key = provider.api_key.trim();
    if raw_key.starts_with("${") && raw_key.ends_with('}') {
        return Err(VoiceError::MissingCredential);
    }
    let mut endpoint = reqwest::Url::parse(provider.base_url.trim())
        .map_err(|_| VoiceError::ProviderUnavailable)?;
    if !matches!(endpoint.scheme(), "http" | "https")
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(VoiceError::ProviderUnavailable);
    }
    let path = format!(
        "{}/audio/transcriptions",
        endpoint.path().trim_end_matches('/')
    );
    endpoint.set_path(&path);
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(timeout.min(Duration::from_secs(10)))
        .timeout(timeout)
        .build()
        .map_err(|_| VoiceError::CannotConnect)?;
    let part = reqwest::multipart::Part::bytes(audio.wav)
        .file_name("speech.wav")
        .mime_str("audio/wav")
        .map_err(|_| VoiceError::InvalidResponse)?;
    let form = reqwest::multipart::Form::new()
        .text("model", model.to_string())
        .part("file", part);
    let mut request = client.post(endpoint).multipart(form);
    if !raw_key.is_empty() {
        request = request.bearer_auth(raw_key);
    }
    let response = request.send().await.map_err(|error| {
        if error.is_timeout() {
            VoiceError::TimedOut
        } else {
            VoiceError::CannotConnect
        }
    })?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(match status {
            401 => VoiceError::AuthenticationRejected,
            403 => VoiceError::AccessDenied,
            404 => VoiceError::EndpointNotFound,
            429 => VoiceError::RateLimited,
            _ => VoiceError::ProviderHttp(status),
        });
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if !matches!(content_type.as_str(), "application/json" | "text/plain") {
        return Err(VoiceError::InvalidResponse);
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| VoiceError::InvalidResponse)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(VoiceError::InvalidResponse);
        }
        bytes.extend_from_slice(&chunk);
    }
    if content_type == "application/json" {
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| VoiceError::InvalidResponse)?;
        let text = value
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or(VoiceError::InvalidResponse)?;
        normalize_transcript(text)
    } else {
        transcript_from_bytes(bytes)
    }
}

fn transcript_from_bytes(bytes: Vec<u8>) -> Result<String, VoiceError> {
    let text = String::from_utf8(bytes).map_err(|_| VoiceError::InvalidResponse)?;
    normalize_transcript(&text)
}

fn normalize_transcript(text: &str) -> Result<String, VoiceError> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_TRANSCRIPT_BYTES || text.contains('\0') {
        return Err(VoiceError::InvalidResponse);
    }
    Ok(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderTimeoutConfig;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn audio() -> VoiceAudio {
        VoiceAudio::test_wav(b"RIFF-safe-test-audio".to_vec())
    }

    async fn server(
        status: &'static str,
        content_type: &'static str,
        body: &'static [u8],
    ) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
        server_owned(status, content_type, body.to_vec()).await
    }

    async fn server_owned(
        status: &'static str,
        content_type: &'static str,
        body: Vec<u8>,
    ) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let count = socket.read(&mut buffer).await.unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
                if let Some(headers_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..headers_end + 4]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(str::trim)
                                .and_then(|value| value.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= headers_end + 4 + length {
                        break;
                    }
                }
            }
            let response = format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.write_all(&body).await.unwrap();
            request
        });
        (format!("http://{address}/v1"), handle)
    }

    fn provider(base_url: String, key: &str) -> ProviderConfig {
        ProviderConfig {
            base_url,
            api_key: key.into(),
            protocol: crate::agent::ProviderProtocol::ChatCompletions,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec![],
            timeout: ProviderTimeoutConfig::default(),
        }
    }

    #[tokio::test]
    async fn provider_transcription_is_one_bounded_multipart_request() {
        let (base_url, request) = server(
            "200 OK",
            "application/json",
            br#"{"text":" editable draft "}"#,
        )
        .await;
        let result = transcribe_provider(
            &provider(base_url, "safe-test-key"),
            "whisper-test",
            Duration::from_secs(2),
            audio(),
        )
        .await
        .unwrap();
        assert_eq!(result, "editable draft");
        let request = request.await.unwrap();
        let request = String::from_utf8_lossy(&request);
        assert!(request.starts_with("POST /v1/audio/transcriptions HTTP/1.1"));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer safe-test-key"));
        assert!(request.contains("whisper-test"));
        assert!(request.contains("RIFF-safe-test-audio"));
    }

    #[tokio::test]
    async fn credentialless_provider_omits_authorization() {
        let (base_url, request) = server("200 OK", "text/plain", b"draft").await;
        assert_eq!(
            transcribe_provider(
                &provider(base_url, ""),
                "local",
                Duration::from_secs(2),
                audio()
            )
            .await
            .unwrap(),
            "draft"
        );
        let request = String::from_utf8_lossy(&request.await.unwrap()).to_ascii_lowercase();
        assert!(!request.contains("authorization:"));
    }

    #[tokio::test]
    async fn provider_failures_are_typed_without_response_bodies() {
        for (status, expected) in [
            ("401 Unauthorized", VoiceError::AuthenticationRejected),
            ("403 Forbidden", VoiceError::AccessDenied),
            ("404 Not Found", VoiceError::EndpointNotFound),
            ("429 Too Many Requests", VoiceError::RateLimited),
            ("500 Internal Server Error", VoiceError::ProviderHttp(500)),
        ] {
            let (base_url, request) = server(status, "text/plain", b"SECRET RESPONSE MARKER").await;
            let error = transcribe_provider(
                &provider(base_url, "key"),
                "model",
                Duration::from_secs(2),
                audio(),
            )
            .await
            .unwrap_err();
            request.await.unwrap();
            assert_eq!(error, expected);
            assert!(!error.to_string().contains("SECRET"));
        }
    }

    #[tokio::test]
    async fn provider_rejects_invalid_and_oversized_responses() {
        for (content_type, body) in [
            ("application/octet-stream", b"unsafe binary".to_vec()),
            ("application/json", b"{not-json}".to_vec()),
            ("text/plain", vec![b'x'; MAX_RESPONSE_BYTES + 1]),
        ] {
            let (base_url, request) = server_owned("200 OK", content_type, body).await;
            let error = transcribe_provider(
                &provider(base_url, "key"),
                "model",
                Duration::from_secs(2),
                audio(),
            )
            .await
            .unwrap_err();
            request.await.unwrap();
            assert_eq!(error, VoiceError::InvalidResponse);
        }
    }

    #[tokio::test]
    async fn missing_credential_and_connection_failure_are_safe() {
        let unresolved = provider("http://127.0.0.1:9/v1".into(), "${MISSING_VOICE_KEY}");
        assert_eq!(
            transcribe_provider(&unresolved, "model", Duration::from_secs(1), audio())
                .await
                .unwrap_err(),
            VoiceError::MissingCredential
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let error = transcribe_provider(
            &provider(format!("http://{address}/v1"), ""),
            "model",
            Duration::from_secs(1),
            audio(),
        )
        .await
        .unwrap_err();
        assert_eq!(error, VoiceError::CannotConnect);
    }

    #[tokio::test]
    async fn provider_transcription_has_a_whole_request_timeout() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            let _ = socket.read(&mut request).await.unwrap();
            tokio::time::sleep(Duration::from_secs(2)).await;
        });
        let error = transcribe_provider(
            &provider(format!("http://{address}/v1"), ""),
            "model",
            Duration::from_millis(50),
            audio(),
        )
        .await
        .unwrap_err();
        assert_eq!(error, VoiceError::TimedOut);
        server.abort();
    }

    #[tokio::test]
    async fn provider_transcription_is_visible_in_the_process_usage_ledger() {
        let (base_url, request) = server("200 OK", "text/plain", b"draft").await;
        let ledger =
            std::sync::Arc::new(std::sync::Mutex::new(crate::usage::UsageLedger::default()));
        let recorder = crate::usage::UsageRecorder::new(
            ledger.clone(),
            crate::usage::UsageKind::VoiceTranscription,
            "local",
            "speech-model",
        );
        let providers = HashMap::from([("local".into(), provider(base_url, ""))]);
        let result = transcribe(
            &VoiceBackendConfig::Provider {
                provider: "local".into(),
                model: "speech-model".into(),
                timeout_secs: 2,
            },
            &providers,
            audio(),
            &[],
            Some(recorder),
        )
        .await
        .unwrap();
        request.await.unwrap();
        assert_eq!(result, "draft");
        let snapshot = ledger.lock().unwrap().snapshot();
        assert_eq!(snapshot.records.len(), 1);
        assert_eq!(
            snapshot.records[0].kind,
            crate::usage::UsageKind::VoiceTranscription
        );
        assert_eq!(
            snapshot.records[0].outcome,
            crate::usage::UsageOutcome::Completed
        );
        assert!(snapshot.records[0].usage.is_none());
    }

    #[test]
    fn transcript_is_trimmed_bounded_and_nul_free() {
        assert_eq!(normalize_transcript(" hello \n").unwrap(), "hello");
        assert_eq!(normalize_transcript(""), Err(VoiceError::InvalidResponse));
        assert_eq!(
            normalize_transcript("bad\0text"),
            Err(VoiceError::InvalidResponse)
        );
        assert_eq!(
            normalize_transcript(&"x".repeat(MAX_TRANSCRIPT_BYTES + 1)),
            Err(VoiceError::InvalidResponse)
        );
    }

    #[test]
    fn voice_debug_state_contains_no_audio() {
        let state = VoiceState::ready(7);
        assert_eq!(
            format!("{state:?}"),
            "VoiceState { phase: Ready, generation: 7, error: None }"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn generated_pcm_is_a_bounded_mono_wav() {
        let audio = encode_wav(vec![0, i16::MAX, i16::MIN, 10], 16_000).unwrap();
        assert!(audio.wav.starts_with(b"RIFF"));
        assert!(audio.wav.len() < 128);
        let mut reader = hound::WavReader::new(std::io::Cursor::new(audio.wav)).unwrap();
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().sample_rate, 16_000);
        assert_eq!(reader.samples::<i16>().count(), 4);
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn local_transcriber_gets_one_temporary_audio_path_without_a_shell() {
        let root = tempfile::tempdir().unwrap();
        let script = root.path().join("transcriber");
        std::fs::write(
            &script,
            "#!/bin/sh\n[ \"$#\" -eq 2 ] || exit 31\n[ \"$1\" = \"--input\" ] || exit 32\n[ -f \"$2\" ] || exit 33\nprintf 'local editable draft'\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&script, permissions).unwrap();

        let transcript = transcribe_local(
            &script,
            &["--input".into(), "{audio}".into()],
            Duration::from_secs(3),
            audio(),
            &[root.path().to_path_buf()],
        )
        .await
        .unwrap();
        assert_eq!(transcript, "local editable draft");
        assert!(std::fs::read_dir(root.path())
            .unwrap()
            .all(|entry| entry.unwrap().path() == script));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn local_transcriber_timeout_kills_its_process_group() {
        let root = tempfile::tempdir().unwrap();
        let script = root.path().join("slow-transcriber");
        let marker = root.path().join("descendant-marker");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n(sleep 2; printf survived > '{}') &\nsleep 30\n",
                marker.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&script, permissions).unwrap();

        let error = transcribe_local(
            &script,
            &["{audio}".into()],
            Duration::from_millis(100),
            audio(),
            &[root.path().to_path_buf()],
        )
        .await
        .unwrap_err();
        assert_eq!(error, VoiceError::TimedOut);
        tokio::time::sleep(Duration::from_millis(2300)).await;
        assert!(!marker.exists());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn cancelling_local_transcription_kills_descendants() {
        let root = tempfile::tempdir().unwrap();
        let script = root.path().join("cancel-transcriber");
        let started = root.path().join("started");
        let descendant = root.path().join("descendant-marker");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf started > '{}'\n(sleep 1; printf survived > '{}') &\nsleep 30\n",
                started.display(),
                descendant.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&script, permissions).unwrap();

        let root_path = root.path().to_path_buf();
        let task = tokio::spawn(async move {
            transcribe_local(
                &script,
                &["{audio}".into()],
                Duration::from_secs(30),
                audio(),
                &[root_path],
            )
            .await
        });
        for _ in 0..100 {
            if started.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(started.exists());
        task.abort();
        let _ = task.await;
        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert!(!descendant.exists());
    }
}
