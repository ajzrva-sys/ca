//! Host side of the offline candidate process. Only the injected host backend
//! can make provider requests; candidate stdout is untrusted framed data.
use crate::agent::client::{
    ChatBackend, ChatMessage, FunctionCall, ToolCallMsg, ToolDef, ToolFunction,
};
use crate::agent::stream::{ProviderEvent, ProviderResponseItem};
use crate::harness_worker::{ProviderReply, WorkerMessage, WorkerSetup, CODING_TOOLS, MAX_FRAME};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct WorkerExecution {
    pub binary: PathBuf,
    pub setup: WorkerSetup,
    pub options: crate::agent::client::ChatRequestOptions,
    pub read_only_roots: Vec<PathBuf>,
    pub denied_paths: Vec<PathBuf>,
    pub deadline: std::time::Duration,
}
pub struct WorkerResult {
    pub exit_code: i32,
    pub final_message: Option<String>,
    pub failure: Option<String>,
    pub provider_requests: u64,
    pub hints_applied: crate::harness_hints::HintPolicy,
}
struct ProcessGroup(Option<i32>);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if let Some(group) = self.0 {
            unsafe {
                libc::kill(-group, libc::SIGKILL);
            }
        }
    }
}

/// Native admission needs a baseline with the current containment behavior.
/// Old V1 candidates remain intact for review and activation recovery, but an
/// older executable cannot attest that future command jobs retain ca's group.
pub(crate) async fn require_native_baseline(binary: &std::path::Path) -> Result<()> {
    if ca_automation::runtime::current_run_store().is_none() {
        return Ok(());
    }
    let binary = binary.to_path_buf();
    let current = std::env::current_exe()?;
    tokio::task::spawn_blocking(move || check_native_baseline(&binary, &current)).await?
}

fn check_native_baseline(binary: &std::path::Path, current: &std::path::Path) -> Result<()> {
    if crate::harness_engineering::artifact_digest(binary)?
        != crate::harness_engineering::artifact_digest(current)?
    {
        bail!("frozen harness baseline cannot attest native worker containment; re-freeze and review the experiment with the current executable before evaluation; existing candidate and evidence are preserved");
    }
    Ok(())
}

pub async fn run_isolated_worker<B: ChatBackend + ?Sized>(
    execution: &WorkerExecution,
    backend: &B,
) -> Result<WorkerResult> {
    execution.setup.validate().map_err(anyhow::Error::msg)?;
    if execution.deadline.is_zero() || execution.deadline > std::time::Duration::from_secs(7200) {
        bail!("invalid worker deadline");
    }
    let root = execution.setup.workspace.canonicalize()?;
    let temporary = tempfile::tempdir()?;
    let home = temporary.path().join("home");
    std::fs::create_dir(&home)?;
    let binary = execution.binary.canonicalize()?;
    let mut read_only_roots = execution.read_only_roots.clone();
    read_only_roots.push(binary.clone());
    let policy = crate::sandbox::SandboxPolicy {
        roots: vec![root.clone(), home.clone()],
        read_only_roots,
        temporary_directory: temporary.path().into(),
        network_allowed: false,
        hide_other_processes: true,
    };
    let prepared = crate::sandbox::SandboxBackend::new(crate::sandbox::SandboxMode::Required)
        .prepare_process_with_denied_paths(
            &binary,
            &[crate::harness_worker::WORKER_FLAG.into()],
            &policy,
            &execution.denied_paths,
        )?;
    let mut command = tokio::process::Command::new(prepared.program);
    command
        .args(prepared.arguments)
        .current_dir(&root)
        .env_clear()
        .env("HOME", &home)
        .env("TMPDIR", temporary.path())
        .env(
            "PATH",
            "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        )
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_HOME", temporary.path().join("cargo-home"))
        .env("CARGO_TARGET_DIR", temporary.path().join("target"))
        .env("CARGO_BUILD_JOBS", "1")
        .env("RUST_TEST_THREADS", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    command.process_group(crate::jobs::owned_child_process_group());
    run_worker_command(
        command,
        &execution.setup,
        &execution.options,
        execution.deadline,
        backend,
    )
    .await
}

/// Shared framed broker for a host sandbox or a host-owned VM transport.
/// The command must already enforce the selected execution boundary. Worker
/// bytes never select the provider, credentials, request options or deadline.
pub(crate) async fn run_worker_command<B: ChatBackend + ?Sized>(
    mut command: tokio::process::Command,
    setup: &WorkerSetup,
    options: &crate::agent::client::ChatRequestOptions,
    deadline: std::time::Duration,
    backend: &B,
) -> Result<WorkerResult> {
    setup.validate().map_err(anyhow::Error::msg)?;
    if deadline.is_zero() || deadline > std::time::Duration::from_secs(7200) {
        bail!("invalid worker deadline");
    }
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .process_group(crate::jobs::owned_child_process_group());
    if crate::jobs::worker_contained() {
        command.env(
            "CA_HARNESS_PARENT_GROUP",
            crate::jobs::owned_child_process_group().to_string(),
        );
    }
    #[cfg(test)]
    if std::env::var_os("CA_HARNESS_TRACE_OUTPUT").as_deref() == Some(std::ffi::OsStr::new("1")) {
        command.stderr(std::process::Stdio::inherit());
    }
    let mut child = command.spawn().context("launch offline harness worker")?;
    let pid = i32::try_from(child.id().context("worker PID unavailable")?)?;
    let group = ProcessGroup((!crate::jobs::worker_contained()).then_some(pid));
    let mut input = child.stdin.take().context("worker input missing")?;
    let mut output = child.stdout.take().context("worker output missing")?;
    let mut setup_frame = Vec::new();
    crate::harness_worker::write_frame(&mut setup_frame, setup)?;
    let operation = async {
        input.write_all(&setup_frame).await?;
        let mut requests = 0;
        let mut hints_applied = crate::harness_hints::HintPolicy::new();
        loop {
            let length = output.read_u32().await? as usize;
            if length == 0 || length > MAX_FRAME {
                bail!("invalid worker output frame length");
            }
            let mut bytes = vec![0; length];
            output.read_exact(&mut bytes).await?;
            let message: WorkerMessage =
                serde_json::from_slice(&bytes).context("invalid worker message")?;
            match message {
                WorkerMessage::Completed {
                    exit_code,
                    final_message,
                    failure,
                } => {
                    drop(input);
                    let status = child.wait().await?;
                    if status.code() != Some(exit_code) {
                        bail!("worker exit disagrees with completion frame");
                    }
                    return Ok(WorkerResult {
                        exit_code,
                        final_message,
                        failure,
                        provider_requests: requests,
                        hints_applied,
                    });
                }
                WorkerMessage::ProviderRequest {
                    sequence,
                    messages,
                    tools,
                } => {
                    if sequence != requests || requests >= setup.maximum_turns as u64 {
                        bail!("worker exceeded frozen request sequence or count");
                    }
                    let (messages, tools) = decode_request(messages, tools)?;
                    if !setup.hints.is_empty() {
                        let reminders = crate::harness_hints::reminders(&messages, &setup.hints);
                        let expected = format!(
                            "{}{}",
                            setup.instructions,
                            crate::harness_hints::reminder_text(&messages, &setup.hints)
                        );
                        let systems = messages
                            .iter()
                            .filter(|message| message.role == "system")
                            .collect::<Vec<_>>();
                        if systems.len() != 1
                            || systems[0].content.as_str() != Some(expected.as_str())
                        {
                            bail!("worker hint transformation differs from its frozen policy");
                        }
                        hints_applied.extend(reminders);
                    }
                    let request = crate::request::PreparedModelRequest::new(
                        crate::request::RequestKind::Headless,
                        setup.provider.clone(),
                        setup.model.clone(),
                        messages,
                        tools,
                        options.clone(),
                    );
                    let mut stream = backend
                        .stream_prepared(&request)
                        .await
                        .context("host provider request failed")?;
                    let mut events = Vec::new();
                    let mut total = 0usize;
                    while let Some(event) = stream.recv().await {
                        let event = event.context("host provider stream failed")?;
                        total = total.saturating_add(serde_json::to_vec(&event)?.len());
                        if total > MAX_FRAME / 2 || events.len() >= 16384 {
                            bail!("host provider response exceeds worker frame bound");
                        }
                        events.push(event);
                    }
                    if !matches!(events.last(), Some(ProviderEvent::Done)) {
                        bail!("host provider response incomplete");
                    }
                    let mut reply = Vec::new();
                    crate::harness_worker::write_frame(
                        &mut reply,
                        &ProviderReply {
                            sequence,
                            events,
                            failed: false,
                        },
                    )?;
                    input.write_all(&reply).await?;
                    requests += 1;
                }
            }
        }
    };
    let result = match tokio::time::timeout(deadline, operation).await {
        Ok(result) => result,
        Err(_) => Err(anyhow::anyhow!("harness worker deadline exhausted")),
    };
    if result.is_err() {
        // Legacy activities signal their child group. Native activities signal
        // only this child; the ca worker owns retirement of the shared group.
        unsafe {
            libc::kill(group.0.map_or(pid, |owned| -owned), libc::SIGTERM);
        }
        if tokio::time::timeout(std::time::Duration::from_secs(3), child.wait())
            .await
            .is_err()
        {
            unsafe {
                libc::kill(group.0.map_or(pid, |owned| -owned), libc::SIGKILL);
            }
            let _ = child.wait().await;
        }
    }
    result
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireMessage {
    role: String,
    content: Option<String>,
    tool_calls: Option<Vec<WireCall>>,
    tool_call_id: Option<String>,
    #[serde(default)]
    response_items: Vec<ProviderResponseItem>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireCall {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    function: WireFunction,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireFunction {
    name: String,
    arguments: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTool {
    #[serde(rename = "type")]
    kind: String,
    function: WireDefinition,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireDefinition {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

fn decode_request(
    messages: Vec<serde_json::Value>,
    tools: Vec<serde_json::Value>,
) -> Result<(Vec<ChatMessage>, Vec<ToolDef>)> {
    if messages.is_empty() || messages.len() > 512 || tools.len() > CODING_TOOLS.len() {
        bail!("worker request surface exceeds bounds");
    }
    let messages = messages
        .into_iter()
        .map(|value| -> Result<ChatMessage> {
            let message: WireMessage = serde_json::from_value(value)?;
            if !matches!(
                message.role.as_str(),
                "system" | "developer" | "user" | "assistant" | "tool"
            ) {
                bail!("invalid worker message role");
            }
            let tool_calls = message.tool_calls.map(|calls| {
                calls
                    .into_iter()
                    .map(|call| ToolCallMsg {
                        id: call.id,
                        call_type: call.kind,
                        function: FunctionCall {
                            name: call.function.name,
                            arguments: call.function.arguments,
                        },
                    })
                    .collect()
            });
            Ok(ChatMessage {
                role: message.role,
                content: crate::image_content::ModelContent::text(
                    message.content.unwrap_or_default(),
                ),
                tool_calls,
                tool_call_id: message.tool_call_id,
                response_items: message.response_items,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut names = std::collections::HashSet::new();
    let tools = tools
        .into_iter()
        .map(|value| -> Result<ToolDef> {
            let tool: WireTool = serde_json::from_value(value)?;
            if tool.kind != "function"
                || !CODING_TOOLS.contains(&tool.function.name.as_str())
                || !names.insert(tool.function.name.clone())
            {
                bail!("worker advertised an ungranted or repeated tool");
            }
            Ok(ToolDef {
                def_type: tool.kind,
                function: ToolFunction {
                    name: tool.function.name,
                    description: tool.function.description,
                    parameters: tool.function.parameters,
                },
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((messages, tools))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aut259_native_baseline_gate_retains_older_executable_without_execution() {
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("older-ca");
        let current = directory.path().join("current-ca");
        std::fs::write(&baseline, b"older contained worker unknown").unwrap();
        std::fs::write(&current, b"current contained worker attested").unwrap();
        let reason = check_native_baseline(&baseline, &current)
            .unwrap_err()
            .to_string();
        assert!(reason.contains("re-freeze and review"));
        assert_eq!(
            std::fs::read(&baseline).unwrap(),
            b"older contained worker unknown"
        );
        std::fs::write(&baseline, std::fs::read(&current).unwrap()).unwrap();
        check_native_baseline(&baseline, &current).unwrap();
    }

    #[test]
    fn hrs253_parent_rejects_credential_and_network_surface_requests() {
        let messages = vec![serde_json::json!({"role":"user","content":"fix this"})];
        let tool = |name| serde_json::json!({"type":"function","function":{"name":name,"description":"test","parameters":{"type":"object"}}});
        assert!(decode_request(messages.clone(), vec![tool("read_file")]).is_ok());
        assert!(decode_request(messages.clone(), vec![tool("web_search")]).is_err());
        assert!(
            decode_request(messages.clone(), vec![tool("read_file"), tool("read_file")]).is_err()
        );
        assert!(decode_request(
            vec![serde_json::json!({"role":"user","content":"fix","api_key":"unexpected"})],
            vec![]
        )
        .is_err());
    }
    struct ProtocolProvider {
        calls: std::sync::atomic::AtomicUsize,
    }
    #[async_trait::async_trait]
    impl ChatBackend for ProtocolProvider {
        async fn stream_chat(
            &self,
            _: &[ChatMessage],
            _: &[ToolDef],
        ) -> Result<
            tokio::sync::mpsc::UnboundedReceiver<crate::agent::client::ChatStreamItem>,
            crate::agent::stream::ProviderError,
        > {
            unreachable!("the broker must construct a frozen prepared request")
        }
        async fn stream_prepared(
            &self,
            request: &crate::request::PreparedModelRequest,
        ) -> Result<
            tokio::sync::mpsc::UnboundedReceiver<crate::agent::client::ChatStreamItem>,
            crate::agent::stream::ProviderError,
        > {
            assert_eq!(request.provider, "host-provider");
            assert_eq!(request.model, "host-model");
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tx.send(Ok(ProviderEvent::Done)).unwrap();
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn hrs253_worker_transport_enforces_frames_counts_exit_and_deadline() {
        let setup = WorkerSetup {
            schema: "ca.harness_worker.v1".into(),
            run_id: uuid::Uuid::new_v4(),
            workspace: "/synthetic".into(),
            provider: "host-provider".into(),
            model: "host-model".into(),
            hints: Default::default(),
            instructions: "test".into(),
            task: "test".into(),
            maximum_turns: 1,
            command_timeout_seconds: 1,
        };
        for (mode, expected_calls, diagnostic) in [
            ("valid", 1, ""),
            ("oversize", 0, "frame length"),
            ("truncated", 0, "early eof"),
            ("sequence", 0, "request sequence"),
            ("unknown", 0, "invalid worker message"),
            ("tool", 0, "ungranted"),
            ("count", 1, "request sequence"),
            ("exit", 1, "exit disagrees"),
            ("hang", 0, "deadline exhausted"),
        ] {
            let mut command = tokio::process::Command::new(crate::harness_vm::HOST_PYTHON);
            command.args([
                "-I",
                "-c",
                include_str!("harness_vm/test_worker_protocol.py"),
                mode,
            ]);
            command.env_clear().env("PATH", "/usr/bin:/bin");
            let provider = ProtocolProvider {
                calls: Default::default(),
            };
            let result = run_worker_command(
                command,
                &setup,
                &Default::default(),
                std::time::Duration::from_millis(if mode == "hang" { 100 } else { 5000 }),
                &provider,
            )
            .await;
            assert_eq!(
                provider.calls.load(std::sync::atomic::Ordering::SeqCst),
                expected_calls,
                "{mode}"
            );
            if diagnostic.is_empty() {
                assert_eq!(result.unwrap().final_message.as_deref(), Some("done"));
            } else {
                let error = result.err().expect(mode).to_string();
                assert!(error.contains(diagnostic), "{mode}: {error}");
            }
        }
    }

    struct FakeProvider {
        calls: std::sync::atomic::AtomicUsize,
        secret: PathBuf,
    }
    #[async_trait::async_trait]
    impl ChatBackend for FakeProvider {
        async fn stream_chat(
            &self,
            messages: &[ChatMessage],
            _: &[ToolDef],
        ) -> Result<
            tokio::sync::mpsc::UnboundedReceiver<crate::agent::client::ChatStreamItem>,
            crate::agent::stream::ProviderError,
        > {
            use crate::agent::stream::FinishReason;
            assert!(!serde_json::to_string(messages)
                .unwrap()
                .contains("private-sentinel-253"));
            let turn = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if turn == 2 {
                let output = messages
                    .iter()
                    .rev()
                    .find(|m| m.role == "tool")
                    .and_then(|m| m.content.as_str())
                    .unwrap();
                assert!(
                    output.contains("private.txt")
                        && (output.contains("Operation not permitted")
                            || output.contains("Permission denied")),
                    "expected an OS denial of the synthetic private file"
                );
            }

            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            let events = if turn < 2 {
                let (name, args) = if turn == 0 {
                    (
                        "write_file",
                        serde_json::json!({"path":"result.txt","content":"verified repair"}),
                    )
                } else {
                    (
                        "run_command",
                        serde_json::json!({"command":format!("cat '{}'",self.secret.to_string_lossy().replace('\'',"'\\''"))}),
                    )
                };
                vec![
                    ProviderEvent::ToolCallStart {
                        index: 0,
                        id: format!("call-{turn}"),
                        name: name.into(),
                    },
                    ProviderEvent::ToolCallArg {
                        index: 0,
                        arg: args.to_string(),
                    },
                    ProviderEvent::Finish {
                        reason: FinishReason::ToolCalls,
                        usage: None,
                    },
                    ProviderEvent::Done,
                ]
            } else {
                vec![
                    ProviderEvent::Text("finished".into()),
                    ProviderEvent::Finish {
                        reason: FinishReason::Stop,
                        usage: None,
                    },
                    ProviderEvent::Done,
                ]
            };
            for event in events {
                sender.send(Ok(event)).unwrap();
            }
            Ok(receiver)
        }
    }
    #[tokio::test]
    #[ignore = "requires freshly built ca and frozen dedicated VM; runs real editing tools through the host broker"]
    async fn hrs253_real_vm_worker_edits_and_hides_frozen_source() {
        use crate::harness_vm::{lease::VmLease, transfer};
        use std::time::{Duration, Instant};
        let binary =
            PathBuf::from(std::env::var_os("CA_HARNESS_TEST_BINARY").expect("fresh worker"));
        let binding = std::env::var_os("CA_HARNESS_VM_TEST_BINDING").expect("VM binding");
        let mut frozen: codex_team::self_improvement::FrozenHarnessVm =
            serde_json::from_slice(&std::fs::read(binding).unwrap()).unwrap();
        frozen.validate().unwrap();
        frozen.adapter_digest = env!("CA_HARNESS_ADAPTER_DIGEST").into();
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::write(workspace.join("input.txt"), "public fixture").unwrap();
        let source = root.path().join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("private.txt"), "private-sentinel-253").unwrap();
        let deadline = Instant::now() + Duration::from_secs(180);
        let package = transfer::package_tree(&workspace, deadline).unwrap();
        let protected = transfer::package_tree(&source, deadline).unwrap();
        let digest = crate::harness_engineering::artifact_digest(&binary).unwrap();
        let lease = VmLease::start(&frozen, Duration::from_secs(180))
            .await
            .unwrap();
        let result: Result<_> = async {
            transfer::install_tree(&lease, &package, transfer::InputKind::Workspace).await?;
            transfer::install_tree(&lease, &protected, transfer::InputKind::Source).await?;
            transfer::install_worker(&lease, &binary, &digest).await?;
            let setup = WorkerSetup {
                schema: "ca.harness_worker.v1".into(),
                run_id: uuid::Uuid::new_v4(),
                workspace,
                provider: "fake".into(),
                model: "fake".into(),
                hints: Default::default(),
                instructions: "Use tools to fix the fixture.".into(),
                task: "Write the requested result.".into(),
                maximum_turns: 4,
                command_timeout_seconds: 2,
            };
            let result = crate::harness_vm::worker::run_prepared_worker(
                &lease,
                &setup,
                &Default::default(),
                Duration::from_secs(30),
                &FakeProvider {
                    calls: Default::default(),
                    secret: "/opt/ca-harness/project/private.txt".into(),
                },
            )
            .await?;
            let output = crate::harness_engineering::execute_capture(
                lease.guest_command(&["/bin/cat", "/opt/ca-harness/task/result.txt"])?,
                lease.remaining(),
            )
            .await?;
            assert!(output.passed);
            assert_eq!(output.out.1, "verified repair");
            Ok(result)
        }
        .await;
        let receipts = lease.finish().await.unwrap();
        eprintln!("REAL_VM_WORKER_CLEANUP={}", receipts.display());
        let result = result.unwrap();
        assert_eq!(result.exit_code, 0, "{:?}", result.failure);
        assert_eq!(result.provider_requests, 3);
        assert_eq!(result.final_message.as_deref(), Some("finished"));
    }

    struct HintProvider {
        calls: std::sync::atomic::AtomicUsize,
        enabled: bool,
    }
    #[async_trait::async_trait]
    impl ChatBackend for HintProvider {
        async fn stream_chat(
            &self,
            messages: &[ChatMessage],
            _: &[ToolDef],
        ) -> Result<
            tokio::sync::mpsc::UnboundedReceiver<crate::agent::client::ChatStreamItem>,
            crate::agent::stream::ProviderError,
        > {
            use crate::agent::stream::FinishReason;
            let turn = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let system = messages
                .iter()
                .find(|m| m.role == "system")
                .unwrap()
                .content
                .as_str()
                .unwrap();
            assert_eq!(
                system.contains("Execution reminder:"),
                self.enabled && turn == 3
            );
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            let events = if turn < 4 {
                vec![ProviderEvent::ToolCallStart { index: 0, id: format!("probe-{turn}"), name: "run_command".into() },
                    ProviderEvent::ToolCallArg { index: 0, arg: serde_json::json!({"command":(["which sh", "which ls", "which cat", "which printf"][turn])}).to_string() },
                    ProviderEvent::Finish { reason: FinishReason::ToolCalls, usage: None }, ProviderEvent::Done]
            } else {
                vec![
                    ProviderEvent::Text("done".into()),
                    ProviderEvent::Finish {
                        reason: FinishReason::Stop,
                        usage: None,
                    },
                    ProviderEvent::Done,
                ]
            };
            for event in events {
                sender.send(Ok(event)).unwrap();
            }
            Ok(receiver)
        }
    }

    #[tokio::test]
    #[ignore = "requires fresh CA_HARNESS_TEST_BINARY and macOS sandbox; verifies real hint delivery"]
    async fn hrs253_real_worker_delivers_only_the_evaluated_hint_at_its_trigger() {
        use crate::harness_hints::ExecutionHint;
        let binary =
            PathBuf::from(std::env::var_os("CA_HARNESS_TEST_BINARY").expect("fresh worker"));
        for enabled in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("workspace");
            std::fs::create_dir(&workspace).unwrap();
            let hints = if enabled {
                [ExecutionHint::DependencyProbes].into_iter().collect()
            } else {
                Default::default()
            };
            let result = run_isolated_worker(
                &WorkerExecution {
                    binary: binary.clone(),
                    setup: WorkerSetup {
                        hints,
                        schema: "ca.harness_worker.v1".into(),
                        run_id: uuid::Uuid::new_v4(),
                        workspace: workspace.canonicalize().unwrap(),
                        provider: "fake".into(),
                        model: "fake".into(),
                        instructions: "Use tools to inspect the fixture.".into(),
                        task: "Inspect the available commands.".into(),
                        maximum_turns: 6,
                        command_timeout_seconds: 2,
                    },
                    options: Default::default(),
                    read_only_roots: Vec::new(),
                    denied_paths: Vec::new(),
                    deadline: std::time::Duration::from_secs(30),
                },
                &HintProvider {
                    calls: std::sync::atomic::AtomicUsize::new(0),
                    enabled,
                },
            )
            .await
            .unwrap();
            assert_eq!(result.exit_code, 0, "{:?}", result.failure);
            assert_eq!(result.provider_requests, 5);
            assert_eq!(
                result
                    .hints_applied
                    .contains(&ExecutionHint::DependencyProbes),
                enabled
            );
        }
    }

    #[tokio::test]
    #[ignore = "requires CA_HARNESS_TEST_BINARY naming a freshly built ca executable and macOS sandbox"]
    async fn hrs253_real_worker_edits_workspace_but_cannot_read_host_private_file() {
        let binary = PathBuf::from(
            std::env::var_os("CA_HARNESS_TEST_BINARY").expect("explicit worker binary"),
        );
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let secret = root.path().join("private.txt");
        std::fs::write(&secret, "private-sentinel-253").unwrap();
        let backend = FakeProvider {
            calls: std::sync::atomic::AtomicUsize::new(0),
            secret: secret.clone(),
        };
        let result = run_isolated_worker(
            &WorkerExecution {
                binary,
                setup: WorkerSetup {
                    hints: Default::default(),
                    schema: "ca.harness_worker.v1".into(),
                    run_id: uuid::Uuid::new_v4(),
                    workspace: workspace.canonicalize().unwrap(),
                    provider: "fake".into(),
                    model: "fake".into(),
                    instructions: "Use tools to fix the fixture.".into(),
                    task: "Write the requested result.".into(),
                    maximum_turns: 4,
                    command_timeout_seconds: 2,
                },
                options: Default::default(),
                read_only_roots: Vec::new(),
                denied_paths: vec![secret],
                deadline: std::time::Duration::from_secs(30),
            },
            &backend,
        )
        .await
        .unwrap();
        assert_eq!(
            result.exit_code, 0,
            "{} provider requests; failure: {:?}",
            result.provider_requests, result.failure
        );
        assert_eq!(result.provider_requests, 3);
        assert_eq!(result.final_message.as_deref(), Some("finished"));
        assert_eq!(
            std::fs::read_to_string(workspace.join("result.txt")).unwrap(),
            "verified repair"
        );
    }
}
