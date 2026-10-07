//! The host retains provider access; the offline guest sees only framed replies.
use super::lease::VmLease;
use crate::agent::client::{ChatBackend, ChatRequestOptions};
use crate::harness_execution::{run_worker_command, WorkerResult};
use crate::harness_worker::WorkerSetup;
use anyhow::Result;
use std::time::Duration;

const SCRIPT: &str = include_str!("worker.py");
const WORKSPACE: &str = "/opt/ca-harness/task";
const PRIVATE: &str = "/Users/ca-runner/harness-worker";

fn profile() -> Result<String> {
    let mut profile = crate::sandbox::sandbox_profile(&crate::sandbox::SandboxPolicy {
        roots: vec![WORKSPACE.into(), PRIVATE.into()],
        read_only_roots: vec!["/opt/homebrew".into()],
        temporary_directory: format!("{PRIVATE}/tmp").into(),
        network_allowed: false,
        hide_other_processes: true,
    })?;
    // The worker is a file in the guest, regardless of host path metadata.
    profile.push_str("(allow file-read* (literal \"/opt/ca-harness/worker\"))\n");
    crate::sandbox::append_denied_path_rules(
        &mut profile,
        &[
            "/opt/ca-harness/project".into(),
            "/opt/ca-harness/project-template".into(),
        ],
    )?;
    Ok(profile)
}

/// The caller installs the hash-bound worker and frozen task first. It owns
/// this lease until any result export completes, and must finish the whole VM
/// before admitting a result. No provider-controlled value enters the RPC argv.
pub(crate) async fn run_prepared_worker<B: ChatBackend + ?Sized>(
    lease: &VmLease,
    setup: &WorkerSetup,
    options: &ChatRequestOptions,
    deadline: Duration,
    backend: &B,
) -> Result<WorkerResult> {
    setup.validate().map_err(anyhow::Error::msg)?;
    let mut setup = setup.clone();
    setup.workspace = WORKSPACE.into();
    let profile = profile()?;
    let command = lease.guest_command_with_input(&[
        "/usr/bin/sudo",
        "-n",
        "/usr/bin/python3",
        "-I",
        "-c",
        SCRIPT,
        &profile,
    ])?;
    run_worker_command(
        command,
        &setup,
        options,
        deadline.min(lease.remaining()),
        backend,
    )
    .await
}

/// Run and export one task in its own VM. Returning requires confirmed whole-VM
/// cleanup; only the bounded, grant-checked bytes cross back to the host.
pub(crate) async fn run_isolated_worker<B: ChatBackend + ?Sized>(
    frozen: &codex_team::self_improvement::FrozenHarnessVm,
    execution: &crate::harness_execution::WorkerExecution,
    expected_binary: &str,
    case: &crate::harness_cases::FrozenCodingCase,
    backend: &B,
) -> Result<(WorkerResult, std::collections::BTreeMap<String, Vec<u8>>)> {
    use super::transfer::{install_tree, install_worker, package_tree, InputKind};
    use anyhow::bail;
    use std::time::Instant;
    execution.setup.validate().map_err(anyhow::Error::msg)?;
    if execution.deadline.is_zero() || execution.deadline > Duration::from_secs(7200) {
        bail!("invalid VM worker allowance");
    }
    let deadline = Instant::now() + execution.deadline;
    let (workspace, input) = (execution.setup.workspace.clone(), case.clone());
    let package = tokio::task::spawn_blocking(move || -> Result<_> {
        if input.check_edits(&workspace)? != input.case.input_digest {
            bail!("VM worker input differs from frozen task");
        }
        let package = package_tree(&workspace, deadline)?;
        if input.check_edits(&workspace)? != input.case.input_digest {
            bail!("VM worker input changed during packaging");
        }
        Ok(package)
    })
    .await??;
    let lease = VmLease::start(frozen, deadline.saturating_duration_since(Instant::now())).await?;
    let operation = async {
        install_tree(&lease, &package, InputKind::Workspace).await?;
        install_worker(&lease, &execution.binary, expected_binary).await?;
        let result = run_prepared_worker(
            &lease,
            &execution.setup,
            &execution.options,
            lease.remaining(),
            backend,
        )
        .await?;
        let files = super::results::fetch_result_files(&lease, case).await?;
        Ok::<_, anyhow::Error>((result, files))
    }
    .await;
    let cleanup = lease.finish().await;
    let result = operation?;
    cleanup?;
    if Instant::now() >= deadline {
        bail!("VM worker allowance exhausted during cleanup");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::{ChatMessage, ChatStreamItem, ToolDef};
    use crate::agent::stream::{ProviderError, ProviderEvent};
    use std::path::Path;
    use std::time::Instant;

    struct Provider;
    #[async_trait::async_trait]
    impl ChatBackend for Provider {
        async fn stream_chat(
            &self,
            messages: &[ChatMessage],
            _: &[ToolDef],
        ) -> Result<tokio::sync::mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].content.as_str(), Some("guest request"));
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tx.send(Ok(ProviderEvent::Text("host reply".into())))
                .unwrap();
            tx.send(Ok(ProviderEvent::Done)).unwrap();
            Ok(rx)
        }
    }

    struct RepairProvider {
        edits: Vec<(String, String)>,
        calls: std::sync::atomic::AtomicUsize,
    }
    #[async_trait::async_trait]
    impl ChatBackend for RepairProvider {
        async fn stream_chat(
            &self,
            _: &[ChatMessage],
            _: &[ToolDef],
        ) -> Result<tokio::sync::mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
            use crate::agent::stream::FinishReason;
            let index = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let events = if let Some((path, content)) = self.edits.get(index) {
                vec![
                    ProviderEvent::ToolCallStart {
                        index: 0,
                        id: format!("edit-{index}"),
                        name: "write_file".into(),
                    },
                    ProviderEvent::ToolCallArg {
                        index: 0,
                        arg: serde_json::json!({"path":path,"content":content}).to_string(),
                    },
                    ProviderEvent::Finish {
                        reason: FinishReason::ToolCalls,
                        usage: None,
                    },
                    ProviderEvent::Done,
                ]
            } else {
                vec![
                    ProviderEvent::Text("{\"completed\":true}".into()),
                    ProviderEvent::Finish {
                        reason: FinishReason::Stop,
                        usage: None,
                    },
                    ProviderEvent::Done,
                ]
            };
            for event in events {
                tx.send(Ok(event)).unwrap();
            }
            Ok(rx)
        }
    }

    #[tokio::test]
    #[ignore = "requires matching fresh ca and dedicated VM; completes edit/export/cleanup and independent verification"]
    async fn hrs253_vm_trial_exports_then_verifies_in_a_fresh_guest() {
        let binding = std::env::var_os("CA_HARNESS_VM_TEST_BINDING").expect("VM binding");
        let mut frozen: codex_team::self_improvement::FrozenHarnessVm =
            serde_json::from_slice(&std::fs::read(binding).unwrap()).unwrap();
        frozen.validate().unwrap();
        frozen.adapter_digest = env!("CA_HARNESS_ADAPTER_DIGEST").into();
        let binary = std::path::PathBuf::from(
            std::env::var_os("CA_HARNESS_TEST_BINARY").expect("matching baseline"),
        );
        let digest = crate::harness_engineering::artifact_digest(&binary).unwrap();
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
        let case = crate::harness_cases::freeze_reasoning_cases(repository)
            .unwrap()
            .remove(0);
        let provider = RepairProvider {
            edits: case
                .allowed_paths
                .iter()
                .map(|name| {
                    (
                        name.clone(),
                        std::fs::read_to_string(
                            repository
                                .join("tests/reasoning_pilot_golden")
                                .join(&case.fixture_id)
                                .join(name),
                        )
                        .unwrap(),
                    )
                })
                .collect(),
            calls: Default::default(),
        };
        let owner = tempfile::tempdir().unwrap();
        let workspace = owner.path().join("task");
        case.materialize(&workspace).unwrap();
        let execution = crate::harness_execution::WorkerExecution {
            binary: binary.clone(),
            setup: WorkerSetup {
                schema: "ca.harness_worker.v1".into(),
                run_id: uuid::Uuid::new_v4(),
                workspace: workspace.clone(),
                provider: "fake".into(),
                model: "fake".into(),
                hints: Default::default(),
                instructions: "Repair the supplied fixture.".into(),
                task: case.task().unwrap().into(),
                maximum_turns: 24,
                command_timeout_seconds: 5,
            },
            options: Default::default(),
            read_only_roots: Vec::new(),
            denied_paths: Vec::new(),
            deadline: Duration::from_secs(120),
        };
        let (result, files) = run_isolated_worker(&frozen, &execution, &digest, &case, &provider)
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0, "{:?}", result.failure);
        assert_eq!(result.provider_requests as usize, provider.edits.len() + 1);
        assert_eq!(
            case.check_edits(&workspace).unwrap(),
            case.case.input_digest
        );
        assert_ne!(files, case.files);
        let imported = owner.path().join("imported");
        super::super::results::materialize_result(&case, &files, &imported).unwrap();
        let verified = super::super::verifier::verify_workspace(
            &frozen,
            &binary,
            &digest,
            &imported,
            &case,
            Duration::from_secs(120),
        )
        .await
        .unwrap();
        assert!(
            verified.public_passed && verified.hidden_passed,
            "{verified:?}"
        );
        assert_eq!(
            verified.implementation_digest,
            case.check_edits(&imported).unwrap()
        );
        eprintln!(
            "VM_COMPLETE_TRIAL={} requests={}",
            case.fixture_id, result.provider_requests
        );
    }

    #[test]
    fn hrs253_vm_worker_profile_excludes_frozen_source_and_network() {
        let profile = profile().unwrap();
        assert!(profile.contains("(deny network*)"));
        assert!(profile.contains("(allow file-read* (literal \"/opt/ca-harness/worker\"))"));
        assert!(
            profile.contains("(deny file-read* file-write* (literal \"/opt/ca-harness/project\")")
        );
        assert!(!profile.contains("(subpath \"/Users/ca-runner\")"));
    }

    #[tokio::test]
    #[ignore = "requires frozen dedicated macOS VM; exercises duplex worker framing and guest sandbox"]
    async fn hrs253_vm_worker_duplex_frames_and_protection() {
        use super::super::transfer::{install_tree, install_worker, package_tree, InputKind};
        let path = std::env::var_os("CA_HARNESS_VM_TEST_BINDING").expect("explicit VM binding");
        let mut frozen: codex_team::self_improvement::FrozenHarnessVm =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        frozen.validate().unwrap();
        // A new fixture uses this adapter with the unchanged, already-hashed image.
        frozen.adapter_digest = env!("CA_HARNESS_ADAPTER_DIGEST").into();
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::write(workspace.join("input.txt"), "frozen task").unwrap();
        let binary = root.path().join("worker");
        std::fs::write(&binary, include_str!("test_worker_fixture.py")).unwrap();
        let hash = crate::harness_engineering::artifact_digest(&binary).unwrap();
        let deadline = Instant::now() + Duration::from_secs(90);
        let package = package_tree(&workspace, deadline).unwrap();
        let lease = VmLease::start(&frozen, Duration::from_secs(90))
            .await
            .unwrap();
        let result = async {
            install_tree(&lease, &package, InputKind::Source).await?;
            install_tree(&lease, &package, InputKind::Workspace).await?;
            install_worker(&lease, &binary, &hash).await?;
            let setup = WorkerSetup {
                schema: "ca.harness_worker.v1".into(),
                run_id: uuid::Uuid::new_v4(),
                workspace: Path::new("/host-only-workspace").into(),
                provider: "fixture".into(),
                model: "fixture".into(),
                hints: Default::default(),
                instructions: "fixture".into(),
                task: "fixture".into(),
                maximum_turns: 2,
                command_timeout_seconds: 2,
            };
            run_prepared_worker(
                &lease,
                &setup,
                &Default::default(),
                Duration::from_secs(20),
                &Provider,
            )
            .await
        }
        .await;
        let receipts = lease.finish().await.unwrap();
        eprintln!("VM_WORKER_CLEANUP={}", receipts.display());
        let result = result.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.provider_requests, 2);
        assert_eq!(
            result.final_message.as_deref(),
            Some("guest duplex verified")
        );
    }
}
