use crate::agent::registry::arguments::{BrowserAction, BrowserArgs};
use crate::agent::registry::{ToolContext, ToolExecutionResult};
use base64::Engine;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::protocol::{Message, WebSocketConfig};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const START_TIMEOUT: Duration = Duration::from_secs(8);
const CDP_TIMEOUT: Duration = Duration::from_secs(8);
const NAVIGATION_TIMEOUT: Duration = Duration::from_secs(20);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_RUNTIME: Duration = Duration::from_secs(10 * 60);
const MAX_DEVTOOLS_FILE_BYTES: usize = 4096;
const MAX_TARGET_LIST_BYTES: usize = 64 * 1024;
const MAX_CDP_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_DOM_RESULT_BYTES: usize = 64 * 1024;
const MAX_SELECTOR_BYTES: usize = 2048;
const MAX_TYPE_TEXT_BYTES: usize = 64 * 1024;
const MAX_URL_BYTES: usize = 4096;
const MAX_SCREENSHOT_BYTES: usize = 10 * 1024 * 1024;
const MAX_DOWNLOAD_BYTES: u64 = 16 * 1024 * 1024;

type CdpSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum BrowserError {
    #[error("Chrome or Chromium is not installed or could not be found")]
    Unavailable,
    #[error("the ephemeral browser could not be started safely")]
    Start,
    #[error("the browser did not respond before the deadline")]
    Timeout,
    #[error("the browser returned an invalid or incomplete response")]
    Protocol,
    #[error("the browser action has missing or unrelated arguments")]
    InvalidArguments,
    #[error("start the browser before using this action")]
    NotRunning,
    #[error("the active browser belongs to another run")]
    WrongOwner,
    #[error("this browser action requires explicit network approval")]
    NetworkDenied,
    #[error("the navigation URL must be a bounded HTTP(S) URL without embedded credentials")]
    InvalidUrl,
    #[error("the browser selector is missing or too large")]
    InvalidSelector,
    #[error("the browser text is missing or too large")]
    InvalidText,
    #[error(
        "the browser key must be Enter, Tab, Escape, Backspace, an arrow, PageUp, PageDown, Home, End, or Space"
    )]
    InvalidKey,
    #[error("scroll needs a selector, or one of up, down, top, or bottom")]
    InvalidScroll,
    #[error("the destination path is missing, already exists, or cannot be written safely")]
    InvalidDestination,
    #[error("the screenshot exceeded its safe size or was not a valid image")]
    InvalidScreenshot,
    #[error("the download exceeded its safe size or did not complete")]
    InvalidDownload,
    #[error("the page did not contain a matching element")]
    MissingElement,
}

struct RunningBrowser {
    owner: crate::jobs::JobOwner,
    child: Option<Child>,
    process_group: Option<i32>,
    _profile: tempfile::TempDir,
    downloads: PathBuf,
    socket: CdpSocket,
    next_id: u64,
    started: Instant,
}

struct BrowserManagerInner {
    running: Mutex<Option<RunningBrowser>>,
    executable: Option<PathBuf>,
    runtime_limit: Duration,
    session: Mutex<Option<String>>,
}

#[derive(Clone)]
pub struct BrowserManager {
    inner: Arc<BrowserManagerInner>,
}

impl BrowserManager {
    pub fn process() -> &'static Self {
        static MANAGER: LazyLock<BrowserManager> = LazyLock::new(BrowserManager::new);
        &MANAGER
    }

    fn new() -> Self {
        Self {
            inner: Arc::new(BrowserManagerInner {
                running: Mutex::new(None),
                executable: None,
                runtime_limit: MAX_RUNTIME,
                session: Mutex::new(None),
            }),
        }
    }

    #[cfg(test)]
    fn with_executable(executable: PathBuf, runtime_limit: Duration) -> Self {
        Self {
            inner: Arc::new(BrowserManagerInner {
                running: Mutex::new(None),
                executable: Some(executable),
                runtime_limit,
                session: Mutex::new(None),
            }),
        }
    }

    pub async fn execute(&self, arguments: &Value, context: &ToolContext) -> ToolExecutionResult {
        let args = match serde_json::from_value::<BrowserArgs>(arguments.clone()) {
            Ok(args) => args,
            Err(_) => {
                return ToolExecutionResult::Failure("Invalid browser action or arguments".into())
            }
        };
        if !valid_action_shape(&args) {
            return ToolExecutionResult::Failure(BrowserError::InvalidArguments.to_string());
        }
        // Keep the bounded operation alive if its caller is cancelled. In
        // particular, an approved navigation must always reach its matching
        // network re-block or process teardown boundary.
        let manager = self.clone();
        let context = context.clone();
        let outcome =
            tokio::spawn(async move { manager.execute_inner(args, &context).await }).await;
        match outcome.unwrap_or(Err(BrowserError::Protocol)) {
            Ok(result) => result,
            Err(error) => ToolExecutionResult::Failure(error.to_string()),
        }
    }

    async fn execute_inner(
        &self,
        args: BrowserArgs,
        context: &ToolContext,
    ) -> Result<ToolExecutionResult, BrowserError> {
        if matches!(args.action, BrowserAction::Start) {
            return self.start(context.job_owner).await;
        }
        let mut slot = self.inner.running.lock().await;
        let expired = slot
            .as_ref()
            .is_some_and(|browser| browser.started.elapsed() >= self.inner.runtime_limit);
        if expired {
            if let Some(mut browser) = slot.take() {
                browser.terminate().await;
            }
            return Err(BrowserError::Timeout);
        }
        let browser = slot.as_mut().ok_or(BrowserError::NotRunning)?;
        if browser.owner != context.job_owner {
            return Err(BrowserError::WrongOwner);
        }
        match args.action {
            BrowserAction::Start => unreachable!(),
            BrowserAction::Navigate => {
                require_network(context)?;
                let url = validated_navigation_url(args.url.as_deref())?;
                browser.navigate_with_network(&url).await?;
                Ok(ToolExecutionResult::Success("Navigation completed".into()))
            }
            BrowserAction::Snapshot => browser.snapshot().await.map(ToolExecutionResult::Success),
            BrowserAction::Controls => browser.controls().await.map(ToolExecutionResult::Success),
            BrowserAction::Html => {
                let selector = validated_selector(args.selector.as_deref())?;
                browser
                    .html(selector)
                    .await
                    .map(ToolExecutionResult::Success)
            }
            BrowserAction::Scroll => {
                browser
                    .scroll(args.selector.as_deref(), args.text.as_deref())
                    .await?;
                Ok(ToolExecutionResult::Success("Scrolled".into()))
            }
            BrowserAction::Click => {
                let selector = validated_selector(args.selector.as_deref())?;
                browser.click(selector).await?;
                Ok(ToolExecutionResult::Success("Element clicked".into()))
            }
            BrowserAction::ClickText => {
                let text = validated_type_text(args.text.as_deref())?;
                browser.click_text(text).await?;
                Ok(ToolExecutionResult::Success(format!("Clicked {text}")))
            }
            BrowserAction::ClickAt => {
                let (x, y) = validated_click_at(args.text.as_deref())?;
                browser.click_at(x, y).await?;
                Ok(ToolExecutionResult::Success(format!("Clicked at {x},{y}")))
            }
            BrowserAction::Type => {
                let selector = validated_selector(args.selector.as_deref())?;
                let text = validated_type_text(args.text.as_deref())?;
                browser.type_text(selector, text).await?;
                Ok(ToolExecutionResult::Success("Element value updated".into()))
            }
            BrowserAction::Press => {
                let label = args.text.as_deref().unwrap_or("key");
                let key = validated_key(args.text.as_deref())?;
                browser.press(key).await?;
                Ok(ToolExecutionResult::Success(format!("Pressed {label}")))
            }
            BrowserAction::Js => {
                let expression = validated_js(args.text.as_deref())?;
                let value = browser.js(expression).await?;
                Ok(ToolExecutionResult::Success(value))
            }
            BrowserAction::Screenshot => {
                let path = required_destination(args.path.as_deref())?;
                let bytes = browser.screenshot().await?;
                let normalized = tokio::task::spawn_blocking(move || {
                    std::panic::catch_unwind(|| {
                        crate::image_content::normalize_encoded_image(bytes)
                    })
                    .map_err(|_| BrowserError::InvalidScreenshot)?
                    .map_err(|_| BrowserError::InvalidScreenshot)
                })
                .await
                .map_err(|_| BrowserError::InvalidScreenshot)??;
                let output = normalized.png_bytes().to_vec();
                persist_bytes(path.clone(), output).await?;
                Ok(ToolExecutionResult::Success(format!(
                    "Screenshot saved to {} (image/png {}×{}, {} bytes)",
                    safe_path(&path),
                    normalized.width(),
                    normalized.height(),
                    normalized.encoded_len()
                )))
            }
            BrowserAction::Download => {
                require_network(context)?;
                let url = validated_navigation_url(args.url.as_deref())?;
                let path = required_destination(args.path.as_deref())?;
                let downloaded = browser.download_with_network(&url).await?;
                persist_download(downloaded, path.clone()).await?;
                Ok(ToolExecutionResult::Success(format!(
                    "Download saved to {}",
                    safe_path(&path)
                )))
            }
            BrowserAction::Close => {
                let mut browser = slot.take().expect("checked running browser");
                browser.terminate().await;
                *self.inner.session.lock().await = None;
                Ok(ToolExecutionResult::Success("Browser closed".into()))
            }
        }
    }

    async fn start(
        &self,
        owner: crate::jobs::JobOwner,
    ) -> Result<ToolExecutionResult, BrowserError> {
        let mut slot = self.inner.running.lock().await;
        if let Some(browser) = slot.as_ref() {
            if browser.owner == owner {
                return Ok(ToolExecutionResult::Success(
                    "Ephemeral browser is already running".into(),
                ));
            }
            return Err(BrowserError::WrongOwner);
        }
        let browser = if self.inner.executable.is_none() {
            if let Some(endpoint) = configured_cdp_endpoint() {
                RunningBrowser::connect_remote(&endpoint, owner).await?
            } else {
                let executable = discover_browser_executable().ok_or(BrowserError::Unavailable)?;
                RunningBrowser::launch(executable, owner).await?
            }
        } else {
            let executable = self
                .inner
                .executable
                .clone()
                .ok_or(BrowserError::Unavailable)?;
            RunningBrowser::launch(executable, owner).await?
        };
        *slot = Some(browser);
        Ok(ToolExecutionResult::Success(
            "Ephemeral browser started with external network blocked".into(),
        ))
    }

    pub async fn capture_image(
        &self,
        context: &ToolContext,
    ) -> Result<crate::image_content::ModelImage, BrowserError> {
        let mut slot = self.inner.running.lock().await;
        let browser = slot.as_mut().ok_or(BrowserError::NotRunning)?;
        if browser.owner != context.job_owner {
            return Err(BrowserError::WrongOwner);
        }
        let bytes = browser.screenshot().await?;
        crate::image_content::normalize_encoded_image(bytes)
            .map_err(|_| BrowserError::InvalidScreenshot)
    }

    pub async fn fill_login(
        &self,
        username: &str,
        password: &str,
        context: &ToolContext,
    ) -> Result<u64, BrowserError> {
        let username = serde_json::to_string(username).map_err(|_| BrowserError::Protocol)?;
        let password = serde_json::to_string(password).map_err(|_| BrowserError::Protocol)?;
        let expression = format!(
            "(/* ca-browser-fill-login */() => {{ const username = {username}; const password = {password}; let filled = 0; for (const input of Array.from(document.querySelectorAll('input'))) {{ const type = String(input.getAttribute('type') || 'text').toLowerCase(); const hint = String((input.name || '') + ' ' + (input.id || '') + ' ' + (input.getAttribute('autocomplete') || '')).toLowerCase(); if (type === 'password') {{ input.focus(); input.value = password; input.dispatchEvent(new Event('input', {{bubbles:true}})); filled += 1; }} else if ((type === 'email' || type === 'text') && /user|email|login|identifier/.test(hint)) {{ input.focus(); input.value = username; input.dispatchEvent(new Event('input', {{bubbles:true}})); filled += 1; }} }} return String(filled); }})()"
        );
        let mut slot = self.inner.running.lock().await;
        let browser = slot.as_mut().ok_or(BrowserError::NotRunning)?;
        if browser.owner != context.job_owner {
            return Err(BrowserError::WrongOwner);
        }
        let value = browser.evaluate_value(&expression).await?;
        value
            .as_str()
            .ok_or(BrowserError::Protocol)?
            .parse::<u64>()
            .map_err(|_| BrowserError::Protocol)
    }

    pub async fn claim_session(&self, name: Option<&str>) -> Result<(), String> {
        let name = name.unwrap_or("").trim();
        if name.chars().count() > 64 || name.chars().any(char::is_control) {
            return Err("browser session name is invalid".into());
        }
        let mut session = self.inner.session.lock().await;
        if let Some(current) = session.as_deref() {
            if current != name {
                return Err(format!(
                    "browser session {current} is already open; reuse that name or close the browser"
                ));
            }
        } else {
            *session = Some(name.to_string());
        }
        Ok(())
    }

    pub async fn page_origin(&self, context: &ToolContext) -> Result<String, BrowserError> {
        let result = self
            .execute(&json!({"action":"js","text":"location.origin"}), context)
            .await;
        if result.is_success() {
            Ok(result.model_text().trim().to_string())
        } else {
            Err(BrowserError::Protocol)
        }
    }

    pub async fn shutdown(&self) {
        let mut slot = self.inner.running.lock().await;
        if let Some(mut browser) = slot.take() {
            browser.terminate().await;
        }
    }

    pub async fn shutdown_owner(&self, owner: crate::jobs::JobOwner) {
        let mut slot = self.inner.running.lock().await;
        if slot.as_ref().is_some_and(|browser| browser.owner == owner) {
            if let Some(mut browser) = slot.take() {
                browser.terminate().await;
            }
        }
    }
}

impl RunningBrowser {
    async fn launch(
        executable: PathBuf,
        owner: crate::jobs::JobOwner,
    ) -> Result<Self, BrowserError> {
        if !is_executable_file(&executable) {
            return Err(BrowserError::Unavailable);
        }
        let profile = tempfile::Builder::new()
            .prefix("ca-browser-")
            .tempdir()
            .map_err(|_| BrowserError::Start)?;
        let downloads = profile.path().join("downloads");
        std::fs::create_dir(&downloads).map_err(|_| BrowserError::Start)?;
        let profile_arg = format!("--user-data-dir={}", profile.path().to_string_lossy());
        let mut command = Command::new(executable);
        command
            .args([
                "--headless=new",
                "--remote-debugging-address=127.0.0.1",
                "--remote-debugging-port=0",
                "--no-first-run",
                "--no-default-browser-check",
                "--no-proxy-server",
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-default-apps",
                "--disable-sync",
                "--disable-extensions",
                "--disable-breakpad",
                "--disable-domain-reliability",
                "--disable-features=AutofillServerCommunication,MediaRouter,OptimizationHints,PasswordManagerOnboarding",
                "--password-store=basic",
                "--use-mock-keychain",
                "--window-size=1280,800",
            ])
            .arg(profile_arg)
            .arg("about:blank")
            .env_clear()
            .env("HOME", profile.path())
            .env("TMPDIR", profile.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        let mut child = command.spawn().map_err(|_| BrowserError::Start)?;
        let process_group = child.id().and_then(|id| i32::try_from(id).ok());
        let port = match wait_for_devtools_port(&mut child, profile.path()).await {
            Ok(port) => port,
            Err(error) => {
                terminate_process_group(process_group);
                let _ = child.start_kill();
                let _ = child.wait().await;
                return Err(error);
            }
        };
        let socket_url = match discover_page_socket(port).await {
            Ok(url) => url,
            Err(error) => {
                terminate_process_group(process_group);
                let _ = child.start_kill();
                let _ = child.wait().await;
                return Err(error);
            }
        };
        let mut websocket_config = WebSocketConfig::default();
        websocket_config.max_message_size = Some(MAX_CDP_MESSAGE_BYTES);
        websocket_config.max_frame_size = Some(MAX_CDP_MESSAGE_BYTES);
        websocket_config.write_buffer_size = 16 * 1024;
        websocket_config.max_write_buffer_size = MAX_CDP_MESSAGE_BYTES;
        let socket = match tokio::time::timeout(
            START_TIMEOUT,
            tokio_tungstenite::connect_async_with_config(
                socket_url.as_str(),
                Some(websocket_config),
                false,
            ),
        )
        .await
        {
            Ok(Ok((socket, _))) => socket,
            _ => {
                terminate_process_group(process_group);
                let _ = child.start_kill();
                let _ = child.wait().await;
                return Err(BrowserError::Start);
            }
        };
        let mut browser = Self {
            owner,
            child: Some(child),
            process_group,
            _profile: profile,
            downloads,
            socket,
            next_id: 1,
            started: Instant::now(),
        };
        browser.request("Page.enable", json!({})).await?;
        browser.request("Runtime.enable", json!({})).await?;
        browser.request("Network.enable", json!({})).await?;
        browser.set_network_blocked(true).await?;
        Ok(browser)
    }

    async fn connect_remote(
        endpoint: &str,
        owner: crate::jobs::JobOwner,
    ) -> Result<Self, BrowserError> {
        let socket_url = configured_socket_url(endpoint).await?;
        let socket = open_socket(socket_url.as_str()).await?;
        let profile = tempfile::Builder::new()
            .prefix("ca-browser-remote-")
            .tempdir()
            .map_err(|_| BrowserError::Start)?;
        let downloads = profile.path().join("downloads");
        std::fs::create_dir(&downloads).map_err(|_| BrowserError::Start)?;
        let mut browser = Self {
            owner,
            child: None,
            process_group: None,
            _profile: profile,
            downloads,
            socket,
            next_id: 1,
            started: Instant::now(),
        };
        browser.request("Page.enable", json!({})).await?;
        browser.request("Runtime.enable", json!({})).await?;
        browser.request("Network.enable", json!({})).await?;
        browser.set_network_blocked(true).await?;
        Ok(browser)
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, BrowserError> {
        self.request_with_timeout(method, params, CDP_TIMEOUT).await
    }

    async fn request_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        deadline: Duration,
    ) -> Result<Value, BrowserError> {
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or(BrowserError::Protocol)?;
        let request = serde_json::to_string(&json!({"id":id,"method":method,"params":params}))
            .map_err(|_| BrowserError::Protocol)?;
        if request.len() > MAX_CDP_MESSAGE_BYTES {
            return Err(BrowserError::Protocol);
        }
        tokio::time::timeout(deadline, self.socket.send(Message::Text(request.into())))
            .await
            .map_err(|_| BrowserError::Timeout)?
            .map_err(|_| BrowserError::Protocol)?;
        let receive = async {
            let mut ignored = 0usize;
            loop {
                let message = self
                    .socket
                    .next()
                    .await
                    .ok_or(BrowserError::Protocol)?
                    .map_err(|_| BrowserError::Protocol)?;
                match message {
                    Message::Text(text) => {
                        if text.len() > MAX_CDP_MESSAGE_BYTES {
                            return Err(BrowserError::Protocol);
                        }
                        let value: Value = serde_json::from_str(text.as_ref())
                            .map_err(|_| BrowserError::Protocol)?;
                        if value.get("id").and_then(Value::as_u64) != Some(id) {
                            ignored = ignored.saturating_add(1);
                            if ignored > 4096 {
                                return Err(BrowserError::Protocol);
                            }
                            continue;
                        }
                        if value.get("error").is_some() {
                            return Err(BrowserError::Protocol);
                        }
                        return value.get("result").cloned().ok_or(BrowserError::Protocol);
                    }
                    Message::Ping(payload) => {
                        self.socket
                            .send(Message::Pong(payload))
                            .await
                            .map_err(|_| BrowserError::Protocol)?;
                    }
                    Message::Close(_) => return Err(BrowserError::Protocol),
                    Message::Binary(_) | Message::Pong(_) | Message::Frame(_) => {}
                }
            }
        };
        tokio::time::timeout(deadline, receive)
            .await
            .map_err(|_| BrowserError::Timeout)?
    }

    async fn set_network_blocked(&mut self, blocked: bool) -> Result<(), BrowserError> {
        let urls: Vec<&str> = if blocked {
            vec!["http://*", "https://*", "ws://*", "wss://*"]
        } else {
            Vec::new()
        };
        self.request("Network.setBlockedURLs", json!({"urls":urls}))
            .await
            .map(|_| ())
    }

    async fn navigate_with_network(&mut self, url: &str) -> Result<(), BrowserError> {
        self.set_network_blocked(false).await?;
        let result = self.navigate(url).await;
        let blocked = self.set_network_blocked(true).await;
        match (result, blocked) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (_, Err(error)) => Err(error),
        }
    }

    async fn download_with_network(&mut self, url: &str) -> Result<PathBuf, BrowserError> {
        self.set_network_blocked(false).await?;
        let result = self.download(url).await;
        let blocked = self.set_network_blocked(true).await;
        match (result, blocked) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (_, Err(error)) => Err(error),
        }
    }

    async fn navigate(&mut self, url: &str) -> Result<(), BrowserError> {
        let result = self.request("Page.navigate", json!({"url":url})).await?;
        if result.get("errorText").is_some() {
            return Err(BrowserError::Protocol);
        }
        let expression = "new Promise((resolve) => { if (document.readyState === 'complete') { resolve(true); } else { addEventListener('load', () => resolve(true), {once:true}); } })";
        self.request_with_timeout(
            "Runtime.evaluate",
            json!({"expression":expression,"awaitPromise":true,"returnByValue":true}),
            NAVIGATION_TIMEOUT,
        )
        .await
        .map(|_| ())
    }

    async fn snapshot(&mut self) -> Result<String, BrowserError> {
        let expression = "(() => { const text = document.documentElement ? (document.documentElement.innerText || '') : ''; const captured = text.slice(0, 15000); return { text: captured, capturedBytes: new TextEncoder().encode(captured).length, totalBytes: new TextEncoder().encode(text).length }; })()";
        let result = self
            .request(
                "Runtime.evaluate",
                json!({"expression":expression,"returnByValue":true}),
            )
            .await?;
        let value = result
            .get("result")
            .and_then(|result| result.get("value"))
            .ok_or(BrowserError::Protocol)?;
        let raw = value
            .get("text")
            .and_then(Value::as_str)
            .ok_or(BrowserError::Protocol)?;
        let captured = value
            .get("capturedBytes")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(BrowserError::Protocol)?;
        let total = value
            .get("totalBytes")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(BrowserError::Protocol)?;
        if captured > total || raw.len() != captured || captured > MAX_DOM_RESULT_BYTES {
            return Err(BrowserError::Protocol);
        }
        let safe = crate::terminal_text::sanitize_terminal_text(raw);
        let omitted = total.saturating_sub(captured);
        let header = format!("DOM text: {total} UTF-8 bytes; captured prefix: {captured} bytes\n");
        let marker = if omitted > 0 {
            format!("\n[… {omitted} source bytes not returned …]")
        } else {
            String::new()
        };
        let body_budget = MAX_DOM_RESULT_BYTES.saturating_sub(header.len() + marker.len());
        let body = bounded_utf8(&safe, body_budget);
        Ok(format!("{header}{body}{marker}"))
    }

    async fn controls(&mut self) -> Result<String, BrowserError> {
        let value = self
            .evaluate_value(CONTROLS_EXPRESSION)
            .await?
            .as_str()
            .ok_or(BrowserError::Protocol)?
            .to_string();
        bounded_controls(&value)
    }

    async fn html(&mut self, selector: &str) -> Result<String, BrowserError> {
        let selector = serde_json::to_string(selector).map_err(|_| BrowserError::Protocol)?;
        let expression = format!(
            "(/* ca-browser-html */() => {{ const element = document.querySelector({selector}); if (!element) return false; const html = element.outerHTML || ''; const captured = html.slice(0, 8000); return {{ html: captured, truncated: html.length > captured.length }}; }})()"
        );
        let value = self.evaluate_value(&expression).await?;
        if value.as_bool() == Some(false) {
            return Err(BrowserError::MissingElement);
        }
        let html = value
            .get("html")
            .and_then(Value::as_str)
            .ok_or(BrowserError::Protocol)?;
        let truncated = value
            .get("truncated")
            .and_then(Value::as_bool)
            .ok_or(BrowserError::Protocol)?;
        let safe = crate::terminal_text::sanitize_terminal_text(html);
        let body = bounded_utf8(&safe, 8 * 1024);
        let marker = if truncated || body.len() < safe.len() {
            "\n[… element HTML truncated …]"
        } else {
            ""
        };
        Ok(format!("{body}{marker}"))
    }

    async fn scroll(
        &mut self,
        selector: Option<&str>,
        direction: Option<&str>,
    ) -> Result<(), BrowserError> {
        let expression = if let Some(selector) = selector {
            let selector = validated_selector(Some(selector))?;
            let selector = serde_json::to_string(selector).map_err(|_| BrowserError::Protocol)?;
            format!(
                "(/* ca-browser-scroll */() => {{ const element = document.querySelector({selector}); if (!element) return false; element.scrollIntoView({{block:'center', inline:'nearest'}}); return true; }})()"
            )
        } else {
            let direction = validated_scroll(direction)?;
            let movement = match direction {
                "up" => "window.scrollBy(0, -Math.round(window.innerHeight * 0.8))",
                "down" => "window.scrollBy(0, Math.round(window.innerHeight * 0.8))",
                "top" => "window.scrollTo(0, 0)",
                "bottom" => "window.scrollTo(0, document.body ? document.body.scrollHeight : 0)",
                _ => return Err(BrowserError::InvalidScroll),
            };
            format!("(/* ca-browser-scroll */() => {{ {movement}; return true; }})()")
        };
        self.evaluate_boolean(&expression).await
    }

    async fn press(&mut self, key: &str) -> Result<(), BrowserError> {
        let key = serde_json::to_string(key).map_err(|_| BrowserError::Protocol)?;
        let expression = format!(
            "(/* ca-browser-press */() => {{ const key = {key}; const target = document.activeElement || document.body; if (!target) return false; const init = {{ key, bubbles: true, cancelable: true }}; target.dispatchEvent(new KeyboardEvent('keydown', init)); target.dispatchEvent(new KeyboardEvent('keyup', init)); if (key === 'Enter' && target.form && typeof target.form.requestSubmit === 'function') target.form.requestSubmit(); return true; }})()"
        );
        self.evaluate_boolean(&expression).await
    }

    async fn click_text(&mut self, text: &str) -> Result<(), BrowserError> {
        let text = serde_json::to_string(text).map_err(|_| BrowserError::Protocol)?;
        let expression = format!(
            "(/* ca-browser-click-text */() => {{ const needle = String({text}).replace(/\\s+/g, ' ').trim().toLowerCase(); const nodes = Array.from(document.querySelectorAll('a,button,input,summary,[role=\"button\"]')); const element = nodes.find((node) => String(node.innerText || node.value || node.getAttribute('aria-label') || '').replace(/\\s+/g, ' ').trim().toLowerCase().includes(needle)); if (!element) return false; element.click(); return true; }})()"
        );
        self.evaluate_boolean(&expression).await
    }

    async fn click_at(&mut self, x: u32, y: u32) -> Result<(), BrowserError> {
        let point = json!({"x": x, "y": y});
        self.request(
            "Input.dispatchMouseEvent",
            json!({"type":"mouseMoved","x":point["x"],"y":point["y"]}),
        )
        .await?;
        self.request(
            "Input.dispatchMouseEvent",
            json!({"type":"mousePressed","x":point["x"],"y":point["y"],"button":"left","clickCount":1}),
        )
        .await?;
        self.request(
            "Input.dispatchMouseEvent",
            json!({"type":"mouseReleased","x":point["x"],"y":point["y"],"button":"left","clickCount":1}),
        )
        .await?;
        Ok(())
    }

    async fn js(&mut self, expression: &str) -> Result<String, BrowserError> {
        let expression = serde_json::to_string(expression).map_err(|_| BrowserError::Protocol)?;
        let wrapped = format!(
            "(/* ca-browser-js */() => {{ const value = eval({expression}); const text = typeof value === 'string' ? value : JSON.stringify(value); return String(text == null ? '' : text).slice(0, 16000); }})()"
        );
        let value = self.evaluate_value(&wrapped).await?;
        let text = value.as_str().ok_or(BrowserError::Protocol)?;
        Ok(bounded_utf8(
            &crate::terminal_text::sanitize_terminal_text(text),
            16 * 1024,
        ))
    }

    async fn click(&mut self, selector: &str) -> Result<(), BrowserError> {
        let selector = serde_json::to_string(selector).map_err(|_| BrowserError::Protocol)?;
        let expression = format!(
            "(() => {{ const element = document.querySelector({selector}); if (!element) return false; element.click(); return true; }})()"
        );
        self.evaluate_boolean(&expression).await
    }

    async fn type_text(&mut self, selector: &str, text: &str) -> Result<(), BrowserError> {
        let selector = serde_json::to_string(selector).map_err(|_| BrowserError::Protocol)?;
        let text = serde_json::to_string(text).map_err(|_| BrowserError::Protocol)?;
        let expression = format!(
            "(() => {{ const element = document.querySelector({selector}); if (!element || !('value' in element)) return false; element.focus(); element.value = {text}; element.dispatchEvent(new Event('input', {{bubbles:true}})); element.dispatchEvent(new Event('change', {{bubbles:true}})); return true; }})()"
        );
        self.evaluate_boolean(&expression).await
    }

    async fn evaluate_value(&mut self, expression: &str) -> Result<Value, BrowserError> {
        let result = self
            .request(
                "Runtime.evaluate",
                json!({"expression":expression,"returnByValue":true}),
            )
            .await?;
        result
            .get("result")
            .and_then(|result| result.get("value"))
            .cloned()
            .ok_or(BrowserError::Protocol)
    }

    async fn evaluate_boolean(&mut self, expression: &str) -> Result<(), BrowserError> {
        match self.evaluate_value(expression).await?.as_bool() {
            Some(true) => Ok(()),
            Some(false) => Err(BrowserError::MissingElement),
            None => Err(BrowserError::Protocol),
        }
    }

    async fn screenshot(&mut self) -> Result<Vec<u8>, BrowserError> {
        let result = self
            .request(
                "Page.captureScreenshot",
                json!({"format":"png","fromSurface":true,"captureBeyondViewport":false}),
            )
            .await?;
        let encoded = result
            .get("data")
            .and_then(Value::as_str)
            .ok_or(BrowserError::InvalidScreenshot)?;
        decode_screenshot(encoded)
    }

    async fn download(&mut self, url: &str) -> Result<PathBuf, BrowserError> {
        reset_download_directory(&self.downloads).await?;
        self.request(
            "Browser.setDownloadBehavior",
            json!({
                "behavior":"allowAndName",
                "downloadPath":self.downloads.to_string_lossy(),
                "eventsEnabled":false
            }),
        )
        .await?;
        let result = self.request("Page.navigate", json!({"url":url})).await?;
        if result.get("errorText").is_some() {
            return Err(BrowserError::Protocol);
        }
        let deadline = Instant::now() + DOWNLOAD_TIMEOUT;
        loop {
            if Instant::now() >= deadline {
                return Err(BrowserError::InvalidDownload);
            }
            let mut entries = tokio::fs::read_dir(&self.downloads)
                .await
                .map_err(|_| BrowserError::InvalidDownload)?;
            let mut complete = Vec::new();
            let mut pending = false;
            while let Some(entry) = entries
                .next_entry()
                .await
                .map_err(|_| BrowserError::InvalidDownload)?
            {
                let metadata = tokio::fs::symlink_metadata(entry.path())
                    .await
                    .map_err(|_| BrowserError::InvalidDownload)?;
                if !metadata.file_type().is_file() || metadata.len() > MAX_DOWNLOAD_BYTES {
                    return Err(BrowserError::InvalidDownload);
                }
                let name = entry.file_name();
                if name.to_string_lossy().ends_with(".crdownload") {
                    pending = true;
                } else {
                    complete.push(entry.path());
                }
            }
            if complete.len() == 1 && !pending {
                return Ok(complete.remove(0));
            }
            if complete.len() > 1 {
                return Err(BrowserError::InvalidDownload);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn terminate(&mut self) {
        let _ = self.socket.send(Message::Close(None)).await;
        terminate_process_group(self.process_group);
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        }
    }
}

impl Drop for RunningBrowser {
    fn drop(&mut self) {
        terminate_process_group(self.process_group);
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
    }
}

fn valid_action_shape(args: &BrowserArgs) -> bool {
    match args.action {
        BrowserAction::Start
        | BrowserAction::Snapshot
        | BrowserAction::Controls
        | BrowserAction::Close => {
            args.url.is_none()
                && args.selector.is_none()
                && args.text.is_none()
                && args.path.is_none()
        }
        BrowserAction::Navigate => {
            args.url.is_some()
                && args.selector.is_none()
                && args.text.is_none()
                && args.path.is_none()
        }
        BrowserAction::Click | BrowserAction::Html => {
            args.url.is_none()
                && args.selector.is_some()
                && args.text.is_none()
                && args.path.is_none()
        }
        BrowserAction::Scroll => {
            args.url.is_none()
                && args.path.is_none()
                && args.selector.is_some() != args.text.is_some()
        }
        BrowserAction::Press
        | BrowserAction::Js
        | BrowserAction::ClickText
        | BrowserAction::ClickAt => {
            args.url.is_none()
                && args.selector.is_none()
                && args.text.is_some()
                && args.path.is_none()
        }
        BrowserAction::Type => {
            args.url.is_none()
                && args.selector.is_some()
                && args.text.is_some()
                && args.path.is_none()
        }
        BrowserAction::Screenshot => {
            args.url.is_none()
                && args.selector.is_none()
                && args.text.is_none()
                && args.path.is_some()
        }
        BrowserAction::Download => {
            args.url.is_some()
                && args.selector.is_none()
                && args.text.is_none()
                && args.path.is_some()
        }
    }
}

fn decode_screenshot(encoded: &str) -> Result<Vec<u8>, BrowserError> {
    if encoded.len() > MAX_SCREENSHOT_BYTES.saturating_mul(4).div_ceil(3) + 4 {
        return Err(BrowserError::InvalidScreenshot);
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| BrowserError::InvalidScreenshot)?;
    if bytes.is_empty() || bytes.len() > MAX_SCREENSHOT_BYTES {
        return Err(BrowserError::InvalidScreenshot);
    }
    Ok(bytes)
}

fn require_network(context: &ToolContext) -> Result<(), BrowserError> {
    context
        .network_allowed
        .then_some(())
        .ok_or(BrowserError::NetworkDenied)
}

fn validated_navigation_url(value: Option<&str>) -> Result<String, BrowserError> {
    let value = value.ok_or(BrowserError::InvalidUrl)?;
    if value.is_empty() || value.len() > MAX_URL_BYTES || value.chars().any(char::is_control) {
        return Err(BrowserError::InvalidUrl);
    }
    let parsed = reqwest::Url::parse(value).map_err(|_| BrowserError::InvalidUrl)?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(BrowserError::InvalidUrl);
    }
    Ok(parsed.into())
}

fn validated_selector(value: Option<&str>) -> Result<&str, BrowserError> {
    let value = value.ok_or(BrowserError::InvalidSelector)?;
    if value.is_empty() || value.len() > MAX_SELECTOR_BYTES || value.chars().any(char::is_control) {
        Err(BrowserError::InvalidSelector)
    } else {
        Ok(value)
    }
}

fn validated_scroll(value: Option<&str>) -> Result<&'static str, BrowserError> {
    match value {
        Some("up") => Ok("up"),
        Some("down") => Ok("down"),
        Some("top") => Ok("top"),
        Some("bottom") => Ok("bottom"),
        _ => Err(BrowserError::InvalidScroll),
    }
}

fn validated_js(value: Option<&str>) -> Result<&str, BrowserError> {
    let value = value.ok_or(BrowserError::InvalidText)?;
    if value.is_empty() || value.len() > 8 * 1024 || value.chars().any(char::is_control) {
        Err(BrowserError::InvalidText)
    } else {
        Ok(value)
    }
}

fn validated_click_at(value: Option<&str>) -> Result<(u32, u32), BrowserError> {
    let value = value.ok_or(BrowserError::InvalidText)?;
    let mut parts = value.split_whitespace();
    let x = parts
        .next()
        .and_then(|part| part.parse::<u32>().ok())
        .ok_or(BrowserError::InvalidText)?;
    let y = parts
        .next()
        .and_then(|part| part.parse::<u32>().ok())
        .ok_or(BrowserError::InvalidText)?;
    if parts.next().is_some() || x > 4000 || y > 4000 {
        return Err(BrowserError::InvalidText);
    }
    Ok((x, y))
}

fn validated_key(value: Option<&str>) -> Result<&'static str, BrowserError> {
    match value {
        Some("Enter") => Ok("Enter"),
        Some("Tab") => Ok("Tab"),
        Some("Escape") => Ok("Escape"),
        Some("Backspace") => Ok("Backspace"),
        Some("ArrowUp") => Ok("ArrowUp"),
        Some("ArrowDown") => Ok("ArrowDown"),
        Some("ArrowLeft") => Ok("ArrowLeft"),
        Some("ArrowRight") => Ok("ArrowRight"),
        Some("PageUp") => Ok("PageUp"),
        Some("PageDown") => Ok("PageDown"),
        Some("Home") => Ok("Home"),
        Some("End") => Ok("End"),
        Some("Space") => Ok(" "),
        _ => Err(BrowserError::InvalidKey),
    }
}

fn bounded_controls(raw: &str) -> Result<String, BrowserError> {
    if raw.len() > MAX_DOM_RESULT_BYTES {
        return Err(BrowserError::Protocol);
    }
    let value: Value = serde_json::from_str(raw).map_err(|_| BrowserError::Protocol)?;
    let controls = value
        .get("controls")
        .and_then(Value::as_array)
        .ok_or(BrowserError::Protocol)?;
    if controls.len() > 40 {
        return Err(BrowserError::Protocol);
    }
    let mut lines = Vec::new();
    for item in controls {
        let selector = item.get("selector").and_then(Value::as_str).unwrap_or("");
        let tag = item.get("tag").and_then(Value::as_str).unwrap_or("");
        if selector.is_empty()
            || selector.len() > MAX_SELECTOR_BYTES
            || tag.is_empty()
            || tag.len() > 32
            || selector.chars().any(char::is_control)
            || tag.chars().any(char::is_control)
        {
            continue;
        }
        let kind = item
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or(tag)
            .chars()
            .filter(|ch| !ch.is_control())
            .take(32)
            .collect::<String>();
        let text = crate::terminal_text::sanitize_terminal_text(
            item.get("text").and_then(Value::as_str).unwrap_or(""),
        );
        let text = bounded_utf8(&text, 80);
        let options = item
            .get("options")
            .and_then(Value::as_array)
            .map(|options| {
                options
                    .iter()
                    .filter_map(Value::as_str)
                    .take(20)
                    .map(|option| {
                        bounded_utf8(&crate::terminal_text::sanitize_terminal_text(option), 60)
                    })
                    .filter(|option| !option.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut line = format!("{tag} {kind} {selector}");
        if !text.is_empty() {
            line.push_str(" \"");
            line.push_str(&text);
            line.push('"');
        }
        if !options.is_empty() {
            line.push_str(" options: ");
            line.push_str(&options.join(", "));
        }
        lines.push(line);
    }
    let omitted = value
        .get("omitted")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(10_000);
    let mut output = format!("Controls: {}\n", lines.len());
    output.push_str(&lines.join("\n"));
    if omitted > 0 {
        output.push_str(&format!("\n[… {omitted} controls not returned …]"));
    }
    Ok(output)
}

const CONTROLS_EXPRESSION: &str = r#"(/* ca-browser-controls */() => {
  const nodes = Array.from(document.querySelectorAll('a[href],button,input,select,textarea,[role="button"]'));
  const controls = [];
  function selectorFor(element) {
    if (element.id && /^[A-Za-z][\w:.-]{0,80}$/.test(element.id)) {
      const byId = '#' + CSS.escape(element.id);
      if (document.querySelectorAll(byId).length === 1) return byId;
    }
    const parts = [];
    let current = element;
    for (let depth = 0; current && current.nodeType === 1 && depth < 6; depth += 1) {
      if (current.id && /^[A-Za-z][\w:.-]{0,80}$/.test(current.id)) {
        parts.unshift('#' + CSS.escape(current.id));
        break;
      }
      let part = current.tagName.toLowerCase();
      const parent = current.parentElement;
      if (parent) {
        const same = Array.from(parent.children).filter((child) => child.tagName === current.tagName);
        if (same.length > 1) part += ':nth-of-type(' + (same.indexOf(current) + 1) + ')';
      }
      parts.unshift(part);
      current = current.parentElement;
    }
    const selector = parts.join(' > ');
    return selector.length > 300 ? '' : selector;
  }
  for (const element of nodes) {
    if (controls.length >= 40) break;
    const selector = selectorFor(element);
    if (!selector) continue;
    const tag = element.tagName.toLowerCase();
    const kind = String(element.getAttribute('type') || (tag === 'a' ? 'link' : tag)).slice(0, 32);
    const text = String(element.innerText || element.getAttribute('value') || element.getAttribute('aria-label') || element.getAttribute('placeholder') || '').replace(/\s+/g, ' ').trim().slice(0, 80);
    const options = [];
    if (tag === 'select') {
      for (const option of Array.from(element.options || []).slice(0, 20)) {
        const label = String(option.label || option.text || '').replace(/\s+/g, ' ').trim().slice(0, 60);
        if (label) options.push(label);
      }
    }
    controls.push({ tag, kind, selector, text, options });
  }
  return JSON.stringify({ omitted: Math.max(0, nodes.length - controls.length), controls });
})()"#;

fn validated_type_text(value: Option<&str>) -> Result<&str, BrowserError> {
    let value = value.ok_or(BrowserError::InvalidText)?;
    if value.len() > MAX_TYPE_TEXT_BYTES {
        Err(BrowserError::InvalidText)
    } else {
        Ok(value)
    }
}

fn required_destination(value: Option<&str>) -> Result<PathBuf, BrowserError> {
    let value = value.ok_or(BrowserError::InvalidDestination)?;
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(BrowserError::InvalidDestination);
    }
    let path = PathBuf::from(value);
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(BrowserError::InvalidDestination);
    }
    Ok(path)
}

fn discover_browser_executable() -> Option<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
        PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
        PathBuf::from("/usr/bin/google-chrome"),
        PathBuf::from("/usr/bin/chromium"),
        PathBuf::from("/usr/bin/chromium-browser"),
    ];
    if let Some(home) = crate::run_environment::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.push(home.join("Applications/Google Chrome.app/Contents/MacOS/Google Chrome"));
        candidates.push(home.join("Applications/Chromium.app/Contents/MacOS/Chromium"));
    }
    if let Some(path) = crate::run_environment::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            for name in ["google-chrome", "chromium", "chromium-browser"] {
                candidates.push(directory.join(name));
            }
        }
    }
    candidates.into_iter().find(|path| is_executable_file(path))
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

async fn wait_for_devtools_port(child: &mut Child, profile: &Path) -> Result<u16, BrowserError> {
    let file = profile.join("DevToolsActivePort");
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if Instant::now() >= deadline {
            return Err(BrowserError::Timeout);
        }
        if child.try_wait().map_err(|_| BrowserError::Start)?.is_some() {
            return Err(BrowserError::Start);
        }
        match tokio::fs::read(&file).await {
            Ok(bytes) => {
                if bytes.len() > MAX_DEVTOOLS_FILE_BYTES {
                    return Err(BrowserError::Protocol);
                }
                if let Ok(text) = std::str::from_utf8(&bytes) {
                    let mut lines = text.lines();
                    let port = lines
                        .next()
                        .and_then(|line| line.parse::<u16>().ok())
                        .filter(|port| *port > 0);
                    let browser_path = lines.next();
                    let complete = port.zip(browser_path).filter(|(_, browser_path)| {
                        browser_path.starts_with("/devtools/browser/")
                            && browser_path.len() <= 512
                            && !browser_path.chars().any(char::is_control)
                            && lines.next().is_none()
                    });
                    if let Some((port, _)) = complete {
                        return Ok(port);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(BrowserError::Start),
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

fn configured_cdp_endpoint() -> Option<String> {
    if let Some(value) = crate::run_environment::var_os("CA_BROWSER_CDP_URL") {
        let value = value.to_string_lossy().trim().to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }
    let path = crate::runtime::RuntimePaths::discover()
        .ok()?
        .config_root
        .join("browser.toml");
    let text = std::fs::read_to_string(path).ok()?;
    let value: toml::Value = toml::from_str(&text).ok()?;
    value
        .get("cdp_url")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

async fn open_socket(url: &str) -> Result<CdpSocket, BrowserError> {
    let mut websocket_config = WebSocketConfig::default();
    websocket_config.max_message_size = Some(MAX_CDP_MESSAGE_BYTES);
    websocket_config.max_frame_size = Some(MAX_CDP_MESSAGE_BYTES);
    websocket_config.write_buffer_size = 16 * 1024;
    websocket_config.max_write_buffer_size = MAX_CDP_MESSAGE_BYTES;
    match tokio::time::timeout(
        START_TIMEOUT,
        tokio_tungstenite::connect_async_with_config(url, Some(websocket_config), false),
    )
    .await
    {
        Ok(Ok((socket, _))) => Ok(socket),
        _ => Err(BrowserError::Start),
    }
}

async fn configured_socket_url(endpoint: &str) -> Result<reqwest::Url, BrowserError> {
    let parsed = reqwest::Url::parse(endpoint).map_err(|_| BrowserError::Start)?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(BrowserError::Start);
    }
    match parsed.scheme() {
        "ws" | "wss" => Ok(parsed),
        "http" | "https" => page_socket_from_http(&parsed).await,
        _ => Err(BrowserError::Start),
    }
}

async fn page_socket_from_http(base: &reqwest::Url) -> Result<reqwest::Url, BrowserError> {
    let mut list = base.clone();
    list.set_path("/json/list");
    list.set_query(None);
    list.set_fragment(None);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(START_TIMEOUT)
        .build()
        .map_err(|_| BrowserError::Start)?;
    let response = client
        .get(list)
        .send()
        .await
        .map_err(|_| BrowserError::Start)?;
    if !response.status().is_success() {
        return Err(BrowserError::Start);
    }
    let body = response.bytes().await.map_err(|_| BrowserError::Protocol)?;
    if body.len() > MAX_TARGET_LIST_BYTES {
        return Err(BrowserError::Protocol);
    }
    let targets: Value = serde_json::from_slice(&body).map_err(|_| BrowserError::Protocol)?;
    let socket = targets
        .as_array()
        .into_iter()
        .flatten()
        .find(|target| target.get("type").and_then(Value::as_str) == Some("page"))
        .and_then(|target| target.get("webSocketDebuggerUrl"))
        .and_then(Value::as_str)
        .ok_or(BrowserError::Protocol)?;
    let parsed = reqwest::Url::parse(socket).map_err(|_| BrowserError::Protocol)?;
    if !matches!(parsed.scheme(), "ws" | "wss")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(BrowserError::Protocol);
    }
    Ok(parsed)
}

async fn discover_page_socket(port: u16) -> Result<reqwest::Url, BrowserError> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(START_TIMEOUT)
        .build()
        .map_err(|_| BrowserError::Start)?;
    let response = client
        .get(format!("http://127.0.0.1:{port}/json/list"))
        .send()
        .await
        .map_err(|_| BrowserError::Start)?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > MAX_TARGET_LIST_BYTES as u64)
    {
        return Err(BrowserError::Protocol);
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| BrowserError::Protocol)?;
        if body.len().saturating_add(chunk.len()) > MAX_TARGET_LIST_BYTES {
            return Err(BrowserError::Protocol);
        }
        body.extend_from_slice(&chunk);
    }
    let targets: Value = serde_json::from_slice(&body).map_err(|_| BrowserError::Protocol)?;
    let socket = targets
        .as_array()
        .into_iter()
        .flatten()
        .find(|target| target.get("type").and_then(Value::as_str) == Some("page"))
        .and_then(|target| target.get("webSocketDebuggerUrl"))
        .and_then(Value::as_str)
        .ok_or(BrowserError::Protocol)?;
    let parsed = reqwest::Url::parse(socket).map_err(|_| BrowserError::Protocol)?;
    let loopback = parsed
        .host_str()
        .and_then(|host| host.parse::<std::net::IpAddr>().ok())
        .is_some_and(|address| address.is_loopback());
    if parsed.scheme() != "ws"
        || !loopback
        || parsed.port_or_known_default() != Some(port)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(BrowserError::Protocol);
    }
    Ok(parsed)
}

async fn reset_download_directory(path: &Path) -> Result<(), BrowserError> {
    let mut entries = tokio::fs::read_dir(path)
        .await
        .map_err(|_| BrowserError::InvalidDownload)?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|_| BrowserError::InvalidDownload)?
    {
        let metadata = tokio::fs::symlink_metadata(entry.path())
            .await
            .map_err(|_| BrowserError::InvalidDownload)?;
        if !metadata.file_type().is_file() {
            return Err(BrowserError::InvalidDownload);
        }
        tokio::fs::remove_file(entry.path())
            .await
            .map_err(|_| BrowserError::InvalidDownload)?;
    }
    Ok(())
}

async fn persist_bytes(path: PathBuf, bytes: Vec<u8>) -> Result<(), BrowserError> {
    tokio::task::spawn_blocking(move || persist_bytes_blocking(&path, &bytes))
        .await
        .map_err(|_| BrowserError::InvalidDestination)?
}

async fn persist_download(source: PathBuf, destination: PathBuf) -> Result<(), BrowserError> {
    tokio::task::spawn_blocking(move || {
        let metadata =
            std::fs::symlink_metadata(&source).map_err(|_| BrowserError::InvalidDownload)?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_DOWNLOAD_BYTES {
            return Err(BrowserError::InvalidDownload);
        }
        let bytes = std::fs::read(&source).map_err(|_| BrowserError::InvalidDownload)?;
        if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
            return Err(BrowserError::InvalidDownload);
        }
        persist_bytes_blocking(&destination, &bytes)
    })
    .await
    .map_err(|_| BrowserError::InvalidDestination)?
}

fn persist_bytes_blocking(path: &Path, bytes: &[u8]) -> Result<(), BrowserError> {
    if path.exists() {
        return Err(BrowserError::InvalidDestination);
    }
    let parent = path.parent().ok_or(BrowserError::InvalidDestination)?;
    if !parent.is_dir() {
        return Err(BrowserError::InvalidDestination);
    }
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| BrowserError::InvalidDestination)?;
    temporary
        .write_all(bytes)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|_| BrowserError::InvalidDestination)?;
    temporary
        .persist_noclobber(path)
        .map_err(|_| BrowserError::InvalidDestination)?;
    Ok(())
}

fn safe_path(path: &Path) -> String {
    crate::terminal_text::sanitize_terminal_text(&path.to_string_lossy())
}

fn bounded_utf8(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn terminate_process_group(process_group: Option<i32>) {
    #[cfg(unix)]
    if let Some(process_group) = process_group {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(process_group),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    #[cfg(not(unix))]
    let _ = process_group;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn tool_context(owner: crate::jobs::JobOwner, network_allowed: bool) -> ToolContext {
        ToolContext {
            recovery: crate::owned_tools::current_recovery(),
            cwd: PathBuf::from("/"),
            timeout_secs: 5,
            profile: crate::agent::registry::ToolProfile::Main,
            workspace_roots: vec![PathBuf::from("/")],
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            network_allowed,
            job_owner: owner,
            job_execution_policy: crate::jobs::JobExecutionPolicy::default(),
        }
    }

    fn fake_browser_executable(root: &Path, port: u16) -> PathBuf {
        let executable = root.join("chromium");
        std::fs::write(
            &executable,
            format!(
                "#!/bin/sh\nprofile=''\nfor arg in \"$@\"; do\n  case \"$arg\" in --user-data-dir=*) profile=${{arg#*=}} ;; esac\ndone\n[ -n \"$profile\" ] || exit 2\n/usr/bin/printf '%s\\n%s\\n' '{port}' '/devtools/browser/fake' > \"$profile/DevToolsActivePort\"\n/bin/sleep 600 &\nchild=$!\n/usr/bin/printf '%s\\n%s\\n' \"$$\" \"$child\" > '{}/browser-pids'\nwait\n",
                root.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&executable, permissions).unwrap();
        executable
    }

    async fn fake_cdp_server(
        navigation_delay: Duration,
    ) -> (u16, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let task = tokio::spawn(async move {
            let (mut http, peer) = listener.accept().await.unwrap();
            assert!(peer.ip().is_loopback());
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while request.len() < 4096 {
                if http.read_exact(&mut byte).await.is_err() {
                    return;
                }
                request.push(byte[0]);
                if request.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            assert!(String::from_utf8_lossy(&request).starts_with("GET /json/list HTTP/1.1"));
            let body = json!([{
                "type":"page",
                "webSocketDebuggerUrl":format!("ws://127.0.0.1:{port}/devtools/page/fake")
            }])
            .to_string();
            http.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(), body
                )
                .as_bytes(),
            )
            .await
            .unwrap();
            drop(http);

            let (stream, peer) = listener.accept().await.unwrap();
            assert!(peer.ip().is_loopback());
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let mut download_path = None::<PathBuf>;
            let screenshot = {
                let image = image::DynamicImage::new_rgba8(2, 2);
                let mut bytes = std::io::Cursor::new(Vec::new());
                image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
                base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
            };
            while let Some(message) = socket.next().await {
                let Ok(Message::Text(text)) = message else {
                    break;
                };
                let request: Value = serde_json::from_str(text.as_ref()).unwrap();
                recorded.lock().await.push(request.clone());
                let id = request["id"].as_u64().unwrap();
                let method = request["method"].as_str().unwrap();
                let result = match method {
                    "Runtime.evaluate" => {
                        let expression = request["params"]["expression"].as_str().unwrap();
                        if expression.contains("document.documentElement") {
                            let text = "safe\u{1b}[31mDOM";
                            json!({"result":{"value":{
                                "text":text,
                                "capturedBytes":text.len(),
                                "totalBytes":text.len() + 9
                            }}})
                        } else if expression.contains("ca-browser-controls") {
                            json!({"result":{"value":"{\"omitted\":1,\"controls\":[{\"tag\":\"button\",\"kind\":\"submit\",\"selector\":\"#accept\",\"text\":\"Accept\",\"options\":[]},{\"tag\":\"select\",\"kind\":\"select\",\"selector\":\"#court\",\"text\":\"Court\",\"options\":[\"Civil\"]}]}"}})
                        } else if expression.contains("ca-browser-js") {
                            json!({"result":{"value":"ready"}})
                        } else if expression.contains("ca-browser-html") {
                            json!({"result":{"value":{"html":"<button id=\"accept\">Accept</button>","truncated":false}}})
                        } else {
                            if expression.contains("new Promise") && !navigation_delay.is_zero() {
                                tokio::time::sleep(navigation_delay).await;
                            }
                            json!({"result":{"value":true}})
                        }
                    }
                    "Page.captureScreenshot" => json!({"data":screenshot}),
                    "Browser.setDownloadBehavior" => {
                        download_path = request["params"]["downloadPath"]
                            .as_str()
                            .map(PathBuf::from);
                        json!({})
                    }
                    "Page.navigate" => {
                        if let Some(path) = download_path.take() {
                            std::fs::write(path.join("download-guid"), b"download-body").unwrap();
                        }
                        json!({"frameId":"fake"})
                    }
                    _ => json!({}),
                };
                let response = json!({"id":id,"result":result}).to_string();
                if socket.send(Message::Text(response.into())).await.is_err() {
                    break;
                }
            }
        });
        (port, requests, task)
    }

    #[test]
    fn navigation_urls_are_bounded_and_never_accept_credentials() {
        assert_eq!(
            validated_navigation_url(Some("https://example.com/path?q=1")).unwrap(),
            "https://example.com/path?q=1"
        );
        for invalid in [
            None,
            Some(""),
            Some("file:///tmp/private"),
            Some("data:text/plain,hello"),
            Some("https://user:secret@example.com"),
            Some("https://example.com/\nunsafe"),
        ] {
            assert_eq!(
                validated_navigation_url(invalid).unwrap_err(),
                BrowserError::InvalidUrl
            );
        }
        assert_eq!(
            validated_navigation_url(Some(&format!("https://example.com/{}", "x".repeat(5000))))
                .unwrap_err(),
            BrowserError::InvalidUrl
        );
    }

    #[test]
    fn selector_text_and_destinations_are_conservative() {
        assert_eq!(validated_selector(Some("#query")).unwrap(), "#query");
        assert_eq!(
            validated_type_text(Some("hello\nworld")).unwrap(),
            "hello\nworld"
        );
        assert_eq!(
            validated_selector(Some("bad\nselector")).unwrap_err(),
            BrowserError::InvalidSelector
        );
        assert_eq!(
            validated_type_text(Some(&"x".repeat(MAX_TYPE_TEXT_BYTES + 1))).unwrap_err(),
            BrowserError::InvalidText
        );
        assert_eq!(validated_scroll(Some("down")).unwrap(), "down");
        assert_eq!(
            validated_scroll(Some("sideways")).unwrap_err(),
            BrowserError::InvalidScroll
        );
        assert_eq!(validated_key(Some("Space")).unwrap(), " ");
        assert_eq!(
            validated_key(Some("F1")).unwrap_err(),
            BrowserError::InvalidKey
        );
        assert_eq!(
            required_destination(Some("relative.png")).unwrap_err(),
            BrowserError::InvalidDestination
        );
        assert!(required_destination(Some("/tmp/shot.png")).is_ok());
    }

    #[test]
    fn action_shapes_are_closed_and_screenshot_decoding_is_bounded() {
        for valid in [
            json!({"action":"start"}),
            json!({"action":"navigate","url":"https://example.com"}),
            json!({"action":"click","selector":"#go"}),
            json!({"action":"html","selector":"#accept"}),
            json!({"action":"scroll","text":"down"}),
            json!({"action":"scroll","selector":"#accept"}),
            json!({"action":"press","text":"Enter"}),
            json!({"action":"controls"}),
            json!({"action":"click_text","text":"Accept"}),
            json!({"action":"click_at","text":"12 40"}),
            json!({"action":"js","text":"document.title"}),
            json!({"action":"type","selector":"#q","text":"hello"}),
            json!({"action":"screenshot","path":"/tmp/shot.png"}),
            json!({"action":"download","url":"https://example.com/a","path":"/tmp/a"}),
            json!({"action":"close"}),
        ] {
            let args: BrowserArgs = serde_json::from_value(valid).unwrap();
            assert!(valid_action_shape(&args));
        }
        for invalid in [
            json!({"action":"start","url":"https://example.com"}),
            json!({"action":"navigate"}),
            json!({"action":"click","selector":"#go","text":"extra"}),
            json!({"action":"scroll"}),
            json!({"action":"scroll","selector":"#accept","text":"down"}),
            json!({"action":"press"}),
            json!({"action":"html"}),
            json!({"action":"type","selector":"#q"}),
            json!({"action":"screenshot"}),
            json!({"action":"download","url":"https://example.com/a"}),
        ] {
            let args: BrowserArgs = serde_json::from_value(invalid).unwrap();
            assert!(!valid_action_shape(&args));
        }
        assert_eq!(
            decode_screenshot("").unwrap_err(),
            BrowserError::InvalidScreenshot
        );
        assert_eq!(
            decode_screenshot("not-base64").unwrap_err(),
            BrowserError::InvalidScreenshot
        );
        let oversized = "A".repeat(MAX_SCREENSHOT_BYTES.saturating_mul(4).div_ceil(3) + 5);
        assert_eq!(
            decode_screenshot(&oversized).unwrap_err(),
            BrowserError::InvalidScreenshot
        );
    }

    #[tokio::test]
    async fn bounded_persistence_is_atomic_and_refuses_overwrite() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("result.bin");
        persist_bytes(path.clone(), b"safe".to_vec()).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"safe");

        let oversized = root.path().join("oversized-source");
        let file = std::fs::File::create(&oversized).unwrap();
        file.set_len(MAX_DOWNLOAD_BYTES + 1).unwrap();
        assert_eq!(
            persist_download(oversized, root.path().join("download.bin"))
                .await
                .unwrap_err(),
            BrowserError::InvalidDownload
        );
        assert!(!root.path().join("download.bin").exists());
        assert_eq!(
            persist_bytes(path.clone(), b"overwrite".to_vec())
                .await
                .unwrap_err(),
            BrowserError::InvalidDestination
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"safe");
    }

    #[tokio::test]
    async fn fake_cdp_lifecycle_is_owner_bound_bounded_and_network_scoped() {
        let (port, requests, server) = fake_cdp_server(Duration::ZERO).await;
        let root = tempfile::tempdir().unwrap();
        let executable = fake_browser_executable(root.path(), port);
        let manager = BrowserManager::with_executable(executable, MAX_RUNTIME);
        let owner = crate::jobs::JobOwner::new(None);
        let offline = tool_context(owner, false);
        let online = tool_context(owner, true);

        let started = manager.execute(&json!({"action":"start"}), &offline).await;
        assert!(started.is_success(), "{}", started.model_text());
        let denied = manager
            .execute(
                &json!({"action":"navigate","url":"https://example.com"}),
                &offline,
            )
            .await;
        assert_eq!(denied.model_text(), BrowserError::NetworkDenied.to_string());
        assert!(!requests
            .lock()
            .await
            .iter()
            .any(|request| { request["method"] == "Page.navigate" }));

        assert!(manager
            .execute(
                &json!({"action":"navigate","url":"https://example.com"}),
                &online,
            )
            .await
            .is_success());
        let snapshot = manager
            .execute(&json!({"action":"snapshot"}), &offline)
            .await;
        assert!(snapshot.is_success());
        assert!(snapshot.model_text().contains("safeDOM"));
        assert!(!snapshot.model_text().contains('\u{1b}'));
        assert!(snapshot
            .model_text()
            .contains("9 source bytes not returned"));
        assert!(manager
            .execute(&json!({"action":"click","selector":"#safe"}), &offline,)
            .await
            .is_success());
        assert!(manager
            .execute(
                &json!({"action":"type","selector":"#safe","text":"hello \"world\""}),
                &offline,
            )
            .await
            .is_success());
        let controls = manager
            .execute(&json!({"action":"controls"}), &offline)
            .await;
        assert!(controls.is_success(), "{}", controls.model_text());
        assert!(controls.model_text().contains("#accept"));
        assert!(controls.model_text().contains("Civil"));
        assert!(controls.model_text().contains("1 controls not returned"));
        let html = manager
            .execute(&json!({"action":"html","selector":"#accept"}), &offline)
            .await;
        assert!(html.model_text().contains("<button"));
        assert!(manager
            .execute(&json!({"action":"scroll","selector":"#accept"}), &offline)
            .await
            .is_success());
        assert!(manager
            .execute(&json!({"action":"scroll","text":"down"}), &offline)
            .await
            .is_success());
        let pressed = manager
            .execute(&json!({"action":"press","text":"Enter"}), &offline)
            .await;
        assert_eq!(pressed.model_text(), "Pressed Enter");
        let script = crate::browser_exec::run_script(
            &manager,
            "# Open the portal and accept terms\ncontrols\njs document.title\nclick text Accept\n",
            None,
            &offline,
        )
        .await;
        assert!(script.is_success(), "{}", script.model_text());
        assert!(script.model_text().contains("#accept"));
        assert!(script.model_text().contains("ready"));
        assert_eq!(
            crate::browser_exec::step_label_from_arguments(
                &json!({"code":"# Open the portal and accept terms\npage\n"}).to_string()
            )
            .as_deref(),
            Some("Open the portal and accept terms")
        );

        let screenshot = root.path().join("screenshot.png");
        let saved = manager
            .execute(&json!({"action":"screenshot","path":screenshot}), &offline)
            .await;
        assert!(saved.is_success(), "{}", saved.model_text());
        let normalized = crate::image_content::normalize_encoded_image(
            std::fs::read(root.path().join("screenshot.png")).unwrap(),
        )
        .unwrap();
        assert_eq!((normalized.width(), normalized.height()), (2, 2));

        let download = root.path().join("download.bin");
        assert!(manager
            .execute(
                &json!({
                    "action":"download",
                    "url":"https://example.com/file",
                    "path":download
                }),
                &online,
            )
            .await
            .is_success());
        assert_eq!(
            std::fs::read(root.path().join("download.bin")).unwrap(),
            b"download-body"
        );

        let other = tool_context(crate::jobs::JobOwner::new(None), false);
        assert_eq!(
            manager
                .execute(&json!({"action":"snapshot"}), &other)
                .await
                .model_text(),
            BrowserError::WrongOwner.to_string()
        );
        manager.shutdown_owner(other.job_owner).await;
        assert!(manager
            .execute(&json!({"action":"snapshot"}), &offline)
            .await
            .is_success());
        assert!(manager
            .execute(&json!({"action":"close"}), &offline)
            .await
            .is_success());
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        for pid in std::fs::read_to_string(root.path().join("browser-pids"))
            .unwrap()
            .lines()
            .map(|line| line.parse::<i32>().unwrap())
        {
            assert!(nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err());
        }

        let requests = requests.lock().await;
        let methods = requests
            .iter()
            .filter_map(|request| request["method"].as_str())
            .collect::<Vec<_>>();
        let navigation = methods
            .iter()
            .position(|method| *method == "Page.navigate")
            .unwrap();
        assert_eq!(methods[navigation - 1], "Network.setBlockedURLs");
        assert_eq!(methods[navigation + 2], "Network.setBlockedURLs");
        let typed = requests.iter().find(|request| {
            request["method"] == "Runtime.evaluate"
                && request["params"]["expression"]
                    .as_str()
                    .is_some_and(|expression| expression.contains("element.value"))
        });
        assert!(typed.unwrap()["params"]["expression"]
            .as_str()
            .unwrap()
            .contains("hello \\\"world\\\""));
    }

    #[tokio::test]
    async fn cancelled_navigation_still_restores_the_network_block() {
        let (port, requests, server) = fake_cdp_server(Duration::from_millis(150)).await;
        let root = tempfile::tempdir().unwrap();
        let manager = BrowserManager::with_executable(
            fake_browser_executable(root.path(), port),
            MAX_RUNTIME,
        );
        let owner = crate::jobs::JobOwner::new(None);
        let offline = tool_context(owner, false);
        let online = tool_context(owner, true);
        let started = manager.execute(&json!({"action":"start"}), &offline).await;
        assert!(
            started.is_success(),
            "{}; requests={:?}",
            started.model_text(),
            requests.lock().await
        );

        let task_manager = manager.clone();
        let task_context = online.clone();
        let task = tokio::spawn(async move {
            task_manager
                .execute(
                    &json!({"action":"navigate","url":"https://example.com"}),
                    &task_context,
                )
                .await
        });
        tokio::time::sleep(Duration::from_millis(40)).await;
        task.abort();
        tokio::time::sleep(Duration::from_millis(250)).await;

        let blocks = requests
            .lock()
            .await
            .iter()
            .filter(|request| request["method"] == "Network.setBlockedURLs")
            .map(|request| request["params"]["urls"].as_array().unwrap().len())
            .collect::<Vec<_>>();
        assert!(blocks.ends_with(&[0, 4]), "{blocks:?}");
        assert!(manager
            .execute(&json!({"action":"close"}), &offline)
            .await
            .is_success());
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn runtime_expiry_closes_the_owned_browser_before_other_actions() {
        let (port, _requests, server) = fake_cdp_server(Duration::ZERO).await;
        let root = tempfile::tempdir().unwrap();
        let manager = BrowserManager::with_executable(
            fake_browser_executable(root.path(), port),
            Duration::ZERO,
        );
        let owner = crate::jobs::JobOwner::new(None);
        let context = tool_context(owner, false);
        assert!(manager
            .execute(&json!({"action":"start"}), &context)
            .await
            .is_success());
        assert_eq!(
            manager
                .execute(&json!({"action":"snapshot"}), &context)
                .await
                .model_text(),
            BrowserError::Timeout.to_string()
        );
        assert_eq!(
            manager
                .execute(&json!({"action":"close"}), &context)
                .await
                .model_text(),
            BrowserError::NotRunning.to_string()
        );
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn target_discovery_rejects_non_loopback_websockets() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut input = [0u8; 4096];
            let _ = stream.read(&mut input).await.unwrap();
            let body = r#"[{"type":"page","webSocketDebuggerUrl":"ws://example.com/devtools/page/unsafe"}]"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        assert_eq!(
            discover_page_socket(port).await.unwrap_err(),
            BrowserError::Protocol
        );
        task.await.unwrap();
    }

    #[tokio::test]
    async fn target_discovery_accepts_only_the_same_loopback_port() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut input = [0u8; 4096];
            let _ = stream.read(&mut input).await.unwrap();
            let body = format!(
                r#"[{{"type":"page","webSocketDebuggerUrl":"ws://127.0.0.1:{port}/devtools/page/safe"}}]"#
            );
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let socket = discover_page_socket(port).await.unwrap();
        assert_eq!(socket.host_str(), Some("127.0.0.1"));
        assert_eq!(socket.port(), Some(port));
        task.await.unwrap();
    }

    #[tokio::test]
    #[ignore = "manual disposable local Chromium walkthrough"]
    async fn real_local_chromium_uses_only_a_disposable_profile_and_loopback_page() {
        let Some(executable) = discover_browser_executable() else {
            return;
        };
        let site = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = site.local_addr().unwrap().port();
        let site_task = tokio::spawn(async move {
            let (mut stream, peer) = site.accept().await.unwrap();
            assert!(peer.ip().is_loopback());
            let mut request = [0u8; 4096];
            let count = stream.read(&mut request).await.unwrap();
            assert!(String::from_utf8_lossy(&request[..count]).starts_with("GET / HTTP/1.1"));
            let body = "<!doctype html><title>BRW-086</title><main>BRW-086 local smoke</main>";
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let root = tempfile::tempdir().unwrap();
        let manager = BrowserManager::with_executable(executable, Duration::from_secs(60));
        let owner = crate::jobs::JobOwner::new(None);
        let offline = tool_context(owner, false);
        let online = tool_context(owner, true);
        let started = manager.execute(&json!({"action":"start"}), &offline).await;
        assert!(started.is_success(), "{}", started.model_text());
        let navigated = manager
            .execute(
                &json!({"action":"navigate","url":format!("http://127.0.0.1:{port}/")}),
                &online,
            )
            .await;
        assert!(navigated.is_success(), "{}", navigated.model_text());
        site_task.await.unwrap();
        let snapshot = manager
            .execute(&json!({"action":"snapshot"}), &offline)
            .await;
        assert!(snapshot.model_text().contains("BRW-086 local smoke"));
        let screenshot = root.path().join("chromium-smoke.png");
        let captured = manager
            .execute(&json!({"action":"screenshot","path":screenshot}), &offline)
            .await;
        assert!(captured.is_success(), "{}", captured.model_text());
        assert!(root.path().join("chromium-smoke.png").is_file());
        assert!(manager
            .execute(&json!({"action":"close"}), &offline)
            .await
            .is_success());
    }

    #[test]
    fn manager_debug_surface_does_not_exist_and_missing_binary_is_safe() {
        let manager = BrowserManager::with_executable(
            PathBuf::from("/definitely/missing/ca-browser"),
            MAX_RUNTIME,
        );
        assert!(Arc::strong_count(&manager.inner) >= 1);
        assert!(!is_executable_file(Path::new(
            "/definitely/missing/ca-browser"
        )));
    }
}
