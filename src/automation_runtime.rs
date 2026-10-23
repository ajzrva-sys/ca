//! ca owns native worker admission and presentation; the engine owns one run.
use crate::automation::{AutomationBudgetV1, AutomationDefinitionV1, AutomationUsageCountersV1};
use crate::runtime::{ActiveProfile, RuntimePaths};
use crate::schedule_v2::{ScheduleDefinitionV2, ScheduleTargetV2};
use ca_automation::native::{AutomationEngine, EngineOperation};
use ca_automation::runtime::{
    ExecutionContext, ExecutionReceipt, ExecutionState, RunStore, EXECUTION_SCHEMA,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

pub fn state_root(paths: &RuntimePaths, profile: &ActiveProfile) -> PathBuf {
    storage_path(&paths.profile_data_dir(profile).join("automation-engine/v2"))
}
pub(crate) fn lease_root(paths: &RuntimePaths) -> PathBuf {
    storage_path(&paths.data_dir.join("automation-resources/v2"))
}
pub(crate) fn private_command_paths(
    paths: &RuntimePaths,
    context: &ExecutionContext,
) -> Vec<PathBuf> {
    vec![
        paths.config_root.clone(),
        context.state_root.clone(),
        context.lease_root.clone(),
    ]
}
/// Resolve aliases for new execution storage, retaining original legacy paths
/// in their contracts and supervisor/session identities.
pub(crate) fn storage_path(path: &std::path::Path) -> PathBuf {
    let mut ancestor = path;
    while !ancestor.exists() {
        let Some(parent) = ancestor.parent() else {
            return path.to_path_buf();
        };
        ancestor = parent;
    }
    match ancestor.canonicalize() {
        Ok(root) => match path.strip_prefix(ancestor) {
            // Joining an empty suffix adds a trailing separator to PathBuf's
            // serialized spelling, changing execution digests after creation.
            Ok(suffix) if suffix.as_os_str().is_empty() => root,
            Ok(suffix) => root.join(suffix),
            Err(_) => path.to_path_buf(),
        },
        Err(_) => path.to_path_buf(),
    }
}

/// Pass only the admitted resource descriptors into the owned worker. The same
/// open descriptions retain their locks if the dispatching parent disappears.
pub(crate) fn inherit_resource_leases(
    command: &mut tokio::process::Command,
    resources: &[ca_automation::runtime::ResourceLease],
) {
    #[cfg(unix)]
    {
        let descriptors = resources
            .iter()
            .map(|lease| lease.raw_fd())
            .collect::<Vec<_>>();
        inherit_resource_descriptors(command, descriptors);
    }
    #[cfg(not(unix))]
    let _ = (command, resources);
}

pub(crate) fn inherit_resource_descriptors(
    command: &mut tokio::process::Command,
    descriptors: Vec<i32>,
) {
    #[cfg(unix)]
    {
        command.env(
            "CA_AUTOMATION_RESOURCE_FDS",
            descriptors
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        unsafe {
            command.pre_exec(move || {
                for descriptor in &descriptors {
                    let flags = libc::fcntl(*descriptor, libc::F_GETFD);
                    if flags < 0
                        || libc::fcntl(*descriptor, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
    }
    #[cfg(not(unix))]
    let _ = (command, descriptors);
}

pub(crate) fn adopt_resource_leases(
    context: &ExecutionContext,
) -> Result<Vec<ca_automation::runtime::ResourceLease>, String> {
    let raw = std::env::var("CA_AUTOMATION_RESOURCE_FDS")
        .map_err(|_| "native resource ownership was not passed to its worker")?;
    let descriptors = if raw.is_empty() {
        Vec::new()
    } else {
        raw.split(',')
            .map(|value| {
                value
                    .parse::<i32>()
                    .map_err(|_| "native resource descriptor is invalid".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    ca_automation::runtime::adopt_resource_leases(context, &descriptors)
        .map_err(|error| error.to_string())
}
fn digest(value: &impl Serialize) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|e| e.to_string())?)
    ))
}

/// Internal V2 worker proof. Legacy definition and approval digests stay intact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeSandboxBinding {
    pub base_policy_digest: String,
    pub mode: String,
}

static ADMITTED_SANDBOX: OnceLock<NativeSandboxBinding> = OnceLock::new();

pub(crate) fn current_sandbox_binding() -> Option<NativeSandboxBinding> {
    ADMITTED_SANDBOX.get().cloned()
}

pub(crate) fn admitted_sandbox_mode(
    configured: crate::sandbox::SandboxMode,
) -> crate::sandbox::SandboxMode {
    ADMITTED_SANDBOX
        .get()
        .and_then(|binding| crate::sandbox::SandboxMode::from_saved_value(&binding.mode).ok())
        .unwrap_or(configured)
}

fn native_policy_base_digest(definition: &AutomationDefinitionV1) -> Result<String, String> {
    digest(&(
        definition.schedule.tools.clone(),
        definition.schedule.grants.clone(),
        definition.schedule.allow_network,
        definition.per_run_budget.clone(),
        definition.pricing.clone(),
    ))
}

fn sandbox_binding(
    definition: &AutomationDefinitionV1,
    mode: crate::sandbox::SandboxMode,
) -> Result<NativeSandboxBinding, String> {
    Ok(NativeSandboxBinding {
        base_policy_digest: native_policy_base_digest(definition)?,
        mode: mode.as_str().into(),
    })
}

fn sandbox_policy_digest(binding: &NativeSandboxBinding) -> Result<String, String> {
    digest(&(&binding.base_policy_digest, &binding.mode))
}

pub(crate) fn validate_sandbox_binding(
    context: &ExecutionContext,
    binding: &NativeSandboxBinding,
) -> Result<crate::sandbox::SandboxMode, String> {
    let mode = crate::sandbox::SandboxMode::from_saved_value(&binding.mode)
        .map_err(|_| "native sandbox mode is invalid".to_string())?;
    if binding.base_policy_digest.len() != 64
        || !binding
            .base_policy_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || context.policy_digest != sandbox_policy_digest(binding)?
    {
        return Err("native sandbox policy changed after admission".into());
    }
    Ok(mode)
}

/// Call only after validating the stored context and inherited ownership. The
/// process is one admitted worker; spawned services retain this frozen policy.
pub(crate) fn activate_sandbox_binding(
    context: &ExecutionContext,
    binding: &NativeSandboxBinding,
) -> Result<(), String> {
    validate_sandbox_binding(context, binding)?;
    if let Some(existing) = ADMITTED_SANDBOX.get() {
        if existing != binding {
            return Err("native worker sandbox ownership changed".into());
        }
        return Ok(());
    }
    ADMITTED_SANDBOX
        .set(binding.clone())
        .map_err(|_| "native worker sandbox was already bound".into())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWorkerSpec {
    pub context: ExecutionContext,
    pub schedule: ScheduleDefinitionV2,
    pub budget: AutomationBudgetV1,
    pub workflow_config: Option<PathBuf>,
    pub definition: AutomationDefinitionV1,
    pub history: Vec<crate::app::Message>,
    pub(crate) sandbox_binding: NativeSandboxBinding,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeResult {
    pub content: String,
    pub usage: AutomationUsageCountersV1,
}
#[cfg(test)]
pub fn execution_context(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    definition: &AutomationDefinitionV1,
    run_id: Uuid,
) -> Result<ExecutionContext, String> {
    let config = crate::config::ConfigStore::for_read_only_load(
        paths.clone().for_profile(profile).config_file,
        profile.clone(),
    )
    .load()
    .map_err(|error| error.to_string())?;
    crate::harness_automation::validate_repository_binding(&definition.schedule, &config)
        .map_err(|error| error.to_string())?;
    let mode = crate::sandbox::SandboxMode::from_saved_value(&config.agent.sandbox_mode)
        .map_err(|error| error.to_string())?;
    execution_context_with_mode(paths, profile, definition, run_id, mode)
}

fn execution_context_with_mode(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    definition: &AutomationDefinitionV1,
    run_id: Uuid,
    mode: crate::sandbox::SandboxMode,
) -> Result<ExecutionContext, String> {
    let target = &definition.schedule.target;
    if let ScheduleTargetV2::Mechanistic { actions } = target {
        let actions = u32::try_from(actions.len())
            .map_err(|_| "Automation action batch exceeds counter range")?;
        if actions > definition.per_run_budget.tool_calls {
            return Err("Automation tool-call budget exhausted before target dispatch".into());
        }
    }
    let mut resources = Vec::new();
    match target {
        ScheduleTargetV2::AutomationWorkflow { .. } => {
            resources.push(format!("workflow:{}",workflow_config(paths,profile)?.canonicalize().map_err(|_|"native workflow configuration is unavailable; configure or migrate its bindings")?.display()));
            resources.push("typed-workflow-mutation".into());
        }
        ScheduleTargetV2::Mechanistic { .. } => resources.push(format!(
            "repository:{}",
            definition
                .schedule
                .workspace
                .canonicalize()
                .map_err(|e| e.to_string())?
                .display()
        )),
        ScheduleTargetV2::HarnessDiscovery { vault_root }
        | ScheduleTargetV2::HarnessEvaluation { vault_root, .. }
        | ScheduleTargetV2::HarnessCycle { vault_root } => {
            resources.extend(
                crate::harness_automation::harness_resource_keys(vault_root)
                    .map_err(|error| error.to_string())?,
            );
            if !matches!(target, ScheduleTargetV2::HarnessDiscovery { .. }) {
                let repository = match target {
                    ScheduleTargetV2::HarnessEvaluation { repository, .. } => repository,
                    _ => &definition.schedule.workspace,
                };
                resources.push(format!(
                    "repository:{}",
                    repository
                        .canonicalize()
                        .map_err(|e| e.to_string())?
                        .display()
                ));
            }
        }
        ScheduleTargetV2::RepositoryTask { task } => resources.push(format!(
            "repository:{}",
            task.repository
                .canonicalize()
                .map_err(|e| e.to_string())?
                .display()
        )),
        ScheduleTargetV2::KnowledgeLearning { vault_root, .. } => {
            resources.push(format!("knowledge-learning:{}", vault_root.display()))
        }
        ScheduleTargetV2::Monitor { .. } => {}
        _ => resources.push(format!(
            "workspace:{}",
            definition
                .schedule
                .workspace
                .canonicalize()
                .map_err(|e| e.to_string())?
                .display()
        )),
    }
    if !matches!(
        target,
        ScheduleTargetV2::Monitor { .. } | ScheduleTargetV2::HarnessDiscovery { .. }
    ) {
        let workspace = definition
            .schedule
            .workspace
            .canonicalize()
            .map_err(|error| error.to_string())?;
        resources.push(format!("workspace:{}", workspace.display()));
        resources.push(format!("repository:{}", workspace.display()));
    }
    resources.sort();
    resources.dedup();
    let binding = match target {
        ScheduleTargetV2::AutomationWorkflow {
            resource_binding_digest,
            ..
        } => resource_binding_digest.clone(),
        _ => digest(target)?,
    };
    Ok(ExecutionContext {
        schema: EXECUTION_SCHEMA.into(),
        run_id: run_id.to_string(),
        profile: profile.label().into(),
        state_root: state_root(paths, profile),
        lease_root: lease_root(paths),
        definition_digest: crate::automation::definition_digest(definition)
            .map_err(|e| e.to_string())?,
        resource_binding_digest: binding,
        policy_digest: sandbox_policy_digest(&sandbox_binding(definition, mode)?)?,
        resources,
        runtime_seconds: definition.per_run_budget.runtime_secs,
        output_bytes: definition.per_run_budget.output_bytes,
    })
}
fn workflow_config(paths: &RuntimePaths, profile: &ActiveProfile) -> Result<PathBuf, String> {
    let native = paths.config_root.join("automation-remote.toml");
    if native.is_file() {
        return native.canonicalize().map_err(|e| e.to_string());
    }
    let cfg = crate::config::ConfigStore::for_profile(
        paths.clone().for_profile(profile).config_file,
        profile.clone(),
    )
    .load()
    .map_err(|e| e.to_string())?;
    Ok(cfg.mcp.mechanistic.remote.config)
}

fn coordinator_sandbox_mode(
    target: &ScheduleTargetV2,
    configured: crate::sandbox::SandboxMode,
) -> crate::sandbox::SandboxMode {
    // macOS cannot apply Seatbelt twice. These trusted ca coordinators launch
    // each untrusted physical command through the original, narrower sandbox;
    // putting their journals/config in that command's sandbox would widen its
    // authority. Process ownership, budgets and scoped services remain here.
    let command_boundary = match target {
        ScheduleTargetV2::Mechanistic { actions } => {
            AutomationEngine::requires_command_isolation(actions)
        }
        ScheduleTargetV2::Prompt { .. }
        | ScheduleTargetV2::Workflow { .. }
        | ScheduleTargetV2::ScriptOnly { .. }
        | ScheduleTargetV2::RepositoryTask { .. }
        | ScheduleTargetV2::HarnessDiscovery { .. }
        | ScheduleTargetV2::HarnessEvaluation { .. }
        | ScheduleTargetV2::HarnessCycle { .. }
        | ScheduleTargetV2::KnowledgeLearning { .. } => true,
        _ => false,
    };
    if command_boundary && configured == crate::sandbox::SandboxMode::Required {
        crate::sandbox::SandboxMode::PolicyOnly
    } else {
        configured
    }
}

pub async fn execute(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    definition: &AutomationDefinitionV1,
    run_id: Uuid,
    history: Vec<crate::app::Message>,
) -> Result<NativeResult, String> {
    let cfg = crate::config::ConfigStore::for_profile(
        paths.clone().for_profile(profile).config_file,
        profile.clone(),
    )
    .load()
    .map_err(|e| e.to_string())?;
    crate::harness_automation::validate_repository_binding(&definition.schedule, &cfg)
        .map_err(|error| error.to_string())?;
    let mode = crate::sandbox::SandboxMode::from_saved_value(&cfg.agent.sandbox_mode)
        .map_err(|error| error.to_string())?;
    let ctx = execution_context_with_mode(paths, profile, definition, run_id, mode)?;
    let store = RunStore::open(&ctx).map_err(|e| e.to_string())?;
    let status = store.reconcile().map_err(|e| e.to_string())?;
    if status.state.terminal() {
        return result_from_receipt(status);
    }
    // Parent ownership survives a worker crash until every descendant and pipe
    // has been drained. The verified private worker delegates these leases.
    let _resource_ownership = store.acquire_resources().map_err(|e| e.to_string())?;
    let spec = NativeWorkerSpec {
        context: ctx.clone(),
        schedule: definition.schedule.clone(),
        budget: definition.per_run_budget.clone(),
        definition: definition.clone(),
        sandbox_binding: sandbox_binding(definition, mode)?,
        history,
        workflow_config: if matches!(
            definition.schedule.target,
            ScheduleTargetV2::AutomationWorkflow { .. }
        ) {
            Some(workflow_config(paths, profile)?)
        } else {
            None
        },
    };
    let encoded = serde_json::to_vec(&spec).map_err(|e| e.to_string())?;
    if encoded.len() > 512 * 1024 {
        return Err("native worker envelope exceeds its bound".into());
    }
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut write_roots = vec![
        ctx.state_root.clone(),
        ctx.lease_root.clone(),
        paths.schedule_private_root(profile),
    ];
    // The inner native agent runner applies exact tool grants. Its containing
    // worker may write the configured repository only for mutating targets.
    if definition.schedule.workspace_policy != crate::schedule_v2::WorkspacePolicy::ExistingReadOnly
    {
        write_roots.push(definition.schedule.workspace.clone());
    }
    let mut read_roots = vec![
        definition.schedule.workspace.clone(),
        paths.config_root.clone(),
        paths.home.join(".codex"),
        executable.clone(),
    ];
    if let Some(c) = &spec.workflow_config {
        read_roots.push(c.clone());
        if definition.schedule.allow_network {
            // The adapter exposes only named roots already bound into the workflow.
            write_roots
                .extend(AutomationEngine::workflow_local_roots(c).map_err(|e| e.to_string())?);
        }
    }
    match &definition.schedule.target {
        ScheduleTargetV2::RepositoryTask { task } => {
            write_roots.push(task.repository.clone());
            write_roots.push(paths.profile_data_dir(profile).join("worktrees"));
            write_roots.push(paths.profile_data_dir(profile).join("acceptance"));
        }
        ScheduleTargetV2::HarnessDiscovery { vault_root }
        | ScheduleTargetV2::HarnessEvaluation { vault_root, .. }
        | ScheduleTargetV2::HarnessCycle { vault_root } => {
            write_roots.push(vault_root.clone());
            write_roots.push(paths.profile_data_dir(profile));
            if let Some(origin) = crate::harness_automation::migration_source(vault_root)
                .map_err(|e| e.to_string())?
            {
                write_roots.push(origin);
            }
            if let Some(repository) = &cfg.harness_improvement.repository {
                read_roots.push(repository.clone());
            }
        }
        ScheduleTargetV2::KnowledgeLearning { vault_root, .. } => {
            write_roots.push(vault_root.clone());
            write_roots.push(paths.data_dir.clone());
            write_roots.push(paths.profile_data_dir(profile));
        }
        _ => {}
    }
    // Native helpers create these private directories before physical dispatch.
    for root in [&ctx.state_root, &ctx.lease_root] {
        ca_automation::runtime::private_dir(root).map_err(|e| e.to_string())?;
    }
    let arguments = vec![
        "automation".into(),
        "--profile".into(),
        profile.label().into(),
        "__worker".into(),
    ];
    let prepared = crate::sandbox::SandboxBackend::new(coordinator_sandbox_mode(
        &definition.schedule.target,
        mode,
    ))
    .prepare_process(
        &executable,
        &arguments,
        &crate::sandbox::SandboxPolicy {
            roots: write_roots,
            read_only_roots: read_roots,
            temporary_directory: temporary.path().to_path_buf(),
            network_allowed: definition.schedule.allow_network,
            hide_other_processes: false,
        },
    )
    .map_err(|e| e.to_string())?;
    let admission = crate::schedule::admit_native_worker(&definition.schedule.workspace).await?;
    let mut command = tokio::process::Command::new(prepared.program);
    command
        .args(prepared.arguments)
        .current_dir(&definition.schedule.workspace)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .env_clear();
    for key in ["HOME", "PATH", "USER", "LOGNAME", "LANG", "LC_ALL"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command.env("TMPDIR", temporary.path());
    let config_store = crate::config::ConfigStore::for_profile(
        paths.clone().for_profile(profile).config_file,
        profile.clone(),
    );
    let mut providers = match &definition.schedule.target {
        ScheduleTargetV2::HarnessDiscovery { .. }
        | ScheduleTargetV2::HarnessEvaluation { .. }
        | ScheduleTargetV2::HarnessCycle { .. } => {
            crate::harness_automation::credential_providers(&definition.schedule, &cfg, profile)
                .map_err(|error| error.to_string())?
        }
        ScheduleTargetV2::KnowledgeLearning {
            vault_root,
            enrollment_digest,
        } => crate::learning_automation::credential_providers(vault_root, enrollment_digest)
            .map_err(|e| e.to_string())?,
        _ => definition.schedule.provider.iter().cloned().collect(),
    };
    if let ScheduleTargetV2::RepositoryTask { task } = &definition.schedule.target {
        providers.push(task.reviewer_route.provider.clone());
    }
    providers.sort();
    providers.dedup();
    let mut credential_values = Vec::new();
    for provider in providers {
        if cfg
            .providers
            .get(&provider)
            .is_some_and(crate::google_auth::is_vertex)
        {
            for key in [
                "GOOGLE_APPLICATION_CREDENTIALS",
                "GOOGLE_CLOUD_QUOTA_PROJECT",
                "GCE_METADATA_HOST",
            ] {
                if let Some(value) = std::env::var_os(key) {
                    command.env(key, value);
                }
            }
        }
        if let Ok(crate::config::ProviderApiKeySource::Environment(key)) =
            config_store.provider_api_key_source(&provider)
        {
            if let Some(value) = std::env::var_os(&key) {
                credential_values.push(value.to_string_lossy().into_owned());
                command.env(key, value);
            }
        }
    }
    if matches!(
        definition.schedule.target,
        ScheduleTargetV2::AutomationWorkflow { .. }
    ) && definition.schedule.allow_network
    {
        for key in cfg.mcp.mechanistic.remote.env.keys() {
            if let Some(value) = std::env::var_os(key) {
                credential_values.push(value.to_string_lossy().into_owned());
                command.env(key, value);
            }
        }
    }
    #[cfg(unix)]
    command.process_group(0);
    inherit_resource_leases(&mut command, &_resource_ownership);
    let child = command.spawn().map_err(|e| e.to_string())?;
    let mut worker = admission.attach(child);
    let mut stdin = worker
        .child_mut()
        .stdin
        .take()
        .ok_or("worker input unavailable")?;
    let stdout = worker
        .child_mut()
        .stdout
        .take()
        .ok_or("worker output unavailable")?;
    let stderr = worker
        .child_mut()
        .stderr
        .take()
        .ok_or("worker diagnostics unavailable")?;
    let write = tokio::spawn(async move {
        stdin.write_all(&encoded).await?;
        stdin.shutdown().await
    });
    let cap = ctx.output_bytes as usize + 256 * 1024;
    let output = tokio::spawn(read_output(stdout, cap));
    let diagnostics = tokio::spawn(read_output(stderr, 64 * 1024));
    let cancel = async {
        loop {
            if store.cancel_requested().unwrap_or(true) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    };
    let outcome = tokio::select! {
        status=worker.child_mut().wait()=>status.map_err(|e|e.to_string()),
        _=tokio::time::sleep(std::time::Duration::from_secs(ctx.runtime_seconds))=>Err("native automation runtime limit reached".into()),
        _=cancel=>Err("native automation cancelled".into()),
    };
    let cleanup = worker
        .retire(!outcome.as_ref().is_ok_and(|status| status.success()))
        .await
        .map_err(|_| "native worker cleanup stopped")?;
    let _ = write.await;
    let output = output.await;
    let diagnostics = match diagnostics.await {
        Ok(Ok(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
        Ok(Err(error)) => format!("Worker diagnostics unavailable: {error}"),
        Err(_) => "Worker diagnostics reader stopped".into(),
    };
    let failure = |reason: &str| {
        crate::automation_engine_cli::worker_failure(reason, &diagnostics, &credential_values)
    };
    let status = match outcome {
        Ok(status) => status,
        Err(error) => {
            let _ = store.reconcile();
            return Err(failure(&error));
        }
    };
    if !status.success() {
        let _ = store.reconcile();
        return Err(failure(&format!(
            "Native automation worker failed ({status})"
        )));
    }
    if cleanup {
        let _ = store.reconcile();
        return Err(failure(
            "Native automation worker left commands active; they were stopped",
        ));
    }
    let bytes = match output {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(error)) => {
            let _ = store.reconcile();
            return Err(failure(&error));
        }
        Err(_) => {
            let _ = store.reconcile();
            return Err(failure("Native automation output reader stopped"));
        }
    };
    let persisted = store
        .status()
        .map_err(|error| failure(&error.to_string()))?;
    match crate::automation_engine_cli::validate_worker_receipt(&status, &bytes, &persisted) {
        Ok(receipt) => result_from_receipt(receipt),
        Err(error) => {
            // Confirmed receipts remain available for recovery; an abnormal
            // worker transport never authorizes another execution attempt.
            let _ = store.reconcile();
            Err(failure(&error))
        }
    }
}
async fn read_output<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut buf = [0u8; 8192];
    let mut exceeded = false;
    loop {
        let n = reader.read(&mut buf).await.map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if n > limit.saturating_sub(bytes.len()) {
            exceeded = true;
        } else if !exceeded {
            bytes.extend_from_slice(&buf[..n]);
        }
    }
    if exceeded {
        Err("native output exceeded admitted capacity".into())
    } else {
        Ok(bytes)
    }
}
pub(crate) fn result_from_receipt(receipt: ExecutionReceipt) -> Result<NativeResult, String> {
    if receipt.state != ExecutionState::Succeeded {
        return Err(receipt.reason.unwrap_or_else(|| {
            format!(
                "native automation {:?}; inspect run {}",
                receipt.state, receipt.run_id
            )
        }));
    }
    let value = receipt
        .result
        .ok_or("native completion has no recorded result")?;
    if let Ok(result) = serde_json::from_value::<NativeResult>(value.clone()) {
        return Ok(result);
    }
    let content = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    let tool_calls = if value.get("schema").and_then(serde_json::Value::as_str)
        == Some(ca_automation_protocol::MECHANISTIC_RECEIPT_SCHEMA)
    {
        value
            .get("action_results")
            .and_then(serde_json::Value::as_array)
            .and_then(|actions| u32::try_from(actions.len()).ok())
            .ok_or("native mechanistic completion has no bounded action count")?
    } else {
        1
    };
    Ok(NativeResult {
        usage: AutomationUsageCountersV1 {
            tool_calls,
            output_bytes: content.len() as u64,
            ..Default::default()
        },
        content,
    })
}
pub async fn run_stdio(paths: &RuntimePaths, profile: &ActiveProfile) -> Result<(), String> {
    crate::jobs::enable_worker_containment()?;
    ca_automation::runtime::enable_worker_containment().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(512 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 512 * 1024 {
        return Err("native input exceeds its bound".into());
    }
    let spec: NativeWorkerSpec =
        serde_json::from_slice(&bytes).map_err(|_| "native input is invalid")?;
    spec.context.validate()?;
    if spec.context.profile != profile.label()
        || spec.context.state_root != state_root(paths, profile)
        || spec.context.lease_root != lease_root(paths)
    {
        return Err("native profile ownership changed".into());
    }
    if RunStore::load_context(&spec.context.state_root, &spec.context.run_id)
        .map_err(|error| error.to_string())?
        != spec.context
    {
        return Err("native recorded execution authority changed".into());
    }
    let _inherited_resources = adopt_resource_leases(&spec.context)?;
    if spec.schedule != spec.definition.schedule
        || spec.budget != spec.definition.per_run_budget
        || spec.context.definition_digest
            != crate::automation::definition_digest(&spec.definition).map_err(|e| e.to_string())?
        || spec.sandbox_binding.base_policy_digest != native_policy_base_digest(&spec.definition)?
    {
        return Err("native worker definition or policy changed".into());
    }
    activate_sandbox_binding(&spec.context, &spec.sandbox_binding)?;
    let mut cfg = crate::config::ConfigStore::for_profile(
        paths.clone().for_profile(profile).config_file,
        profile.clone(),
    )
    .load()
    .map_err(|e| e.to_string())?;
    cfg.agent.sandbox_mode = spec.sandbox_binding.mode.clone();
    crate::harness_automation::validate_repository_binding(&spec.schedule, &cfg)
        .map_err(|error| error.to_string())?;
    let receipt = match &spec.schedule.target {
        ScheduleTargetV2::AutomationWorkflow {
            workflow_id,
            workflow_digest,
            resource_binding_digest,
        } => {
            if !spec.schedule.allow_network {
                return Err("workflow requires its explicit network grant".into());
            }
            let operation = EngineOperation::Workflow {
                config_path: spec
                    .workflow_config
                    .clone()
                    .ok_or("workflow config is missing")?,
                request: ca_automation_protocol::WorkflowRequestV1 {
                    schema: ca_automation_protocol::WORKFLOW_REQUEST_SCHEMA.into(),
                    operation: ca_automation_protocol::WorkflowOperationV1::Run {
                        workflow_id: workflow_id.clone(),
                        draft: None,
                        accepted_plan_digest: None,
                        expected_workflow_digest: Some(workflow_digest.clone()),
                        expected_resource_binding_digest: Some(resource_binding_digest.clone()),
                    },
                },
            };
            AutomationEngine::start(&spec.context, operation)
                .await
                .map_err(|e| e.to_string())?
        }
        ScheduleTargetV2::Mechanistic { actions } => {
            let revision = AutomationEngine::repository_revision(&spec.schedule.workspace)
                .map_err(|e| e.to_string())?;
            let request = ca_automation_protocol::MechanisticRequestV1 {
                schema: ca_automation_protocol::MECHANISTIC_REQUEST_SCHEMA.into(),
                expected_revision: revision,
                actions: actions.clone(),
                // Native verification always owns a required command sandbox;
                // the legacy external-delegation advisory flag grants no
                // authority to a ca automation.
                allow_unisolated_evidence: false,
                delegation_policy: ca_automation_protocol::DelegationPolicy::Require,
                purpose: ca_automation_protocol::MechanisticRequestPurposeV1::Ordinary,
            };
            AutomationEngine::start(
                &spec.context,
                EngineOperation::Mechanistic {
                    repository: spec.schedule.workspace.clone(),
                    request,
                },
            )
            .await
            .map_err(|e| e.to_string())?
        }
        _ => {
            crate::jobs::AUTOMATION_PRIVATE_PATHS
                .scope(
                    private_command_paths(paths, &spec.context),
                    run_host_target(paths, profile, &cfg, &spec),
                )
                .await?
        }
    };
    println!(
        "{}",
        serde_json::to_string(&receipt).map_err(|e| e.to_string())?
    );
    Ok(())
}
async fn run_host_target(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    cfg: &crate::config::CaConfig,
    spec: &NativeWorkerSpec,
) -> Result<ExecutionReceipt, String> {
    let store = RunStore::open(&spec.context).map_err(|e| e.to_string())?;
    let _owner = store.execution_lease().map_err(|e| e.to_string())?;
    let _resources = store.acquire_resources().map_err(|e| e.to_string())?;
    let status = store.start().map_err(|e| e.to_string())?;
    if status.state.terminal() {
        return Ok(status);
    }
    store
        .prepare_action("target")
        .and_then(|_| store.begin_action("target"))
        .map_err(|e| e.to_string())?;
    let result: Result<NativeResult, String> = ca_automation::runtime::EXECUTION_STORE
        .scope(
            store.clone(),
            Box::pin(async {
                match &spec.schedule.target {
                    ScheduleTargetV2::RepositoryTask { task } => {
                        match crate::repository_automation::run(
                            task,
                            &spec.context,
                            paths,
                            profile,
                            &spec.budget,
                            &spec.schedule.tools,
                            &spec.schedule.grants,
                            spec.schedule.allow_network,
                        )
                        .await
                        {
                            Ok(outcome) => {
                                let usage = outcome.usage.clone();
                                let mut result = model_free_result(
                                    serde_json::to_value(outcome).map_err(|e| e.to_string())?,
                                );
                                result.usage = usage;
                                Ok(result)
                            }
                            Err(error) => Err(error),
                        }
                    }
                    ScheduleTargetV2::HarnessDiscovery { vault_root } => {
                        crate::harness_automation::discover(
                            paths.profile_data_dir(profile),
                            vault_root.clone(),
                        )
                        .await
                        .map(model_free_result)
                        .map_err(|e| e.to_string())
                    }
                    ScheduleTargetV2::HarnessEvaluation {
                        vault_root,
                        cycle_id,
                        expected_plan_digest,
                        repository,
                        baseline_binary,
                    } => crate::harness_automation::with_budget(
                        spec.budget.clone(),
                        crate::harness_automation::evaluate(
                            vault_root.clone(),
                            cycle_id.clone(),
                            expected_plan_digest.clone(),
                            repository.clone(),
                            baseline_binary.clone(),
                            cfg.clone(),
                        ),
                    )
                    .await
                    .map(|v| harness_result(v, &spec.budget))
                    .map_err(|e| e.to_string()),
                    ScheduleTargetV2::HarnessCycle { vault_root } => {
                        crate::harness_automation::with_budget(
                            spec.budget.clone(),
                            crate::harness_automation::cycle(
                                paths.clone(),
                                profile.clone(),
                                cfg.clone(),
                                vault_root.clone(),
                            ),
                        )
                        .await
                        .map(|v| harness_result(v, &spec.budget))
                        .map_err(|e| e.to_string())
                    }
                    ScheduleTargetV2::KnowledgeLearning {
                        vault_root,
                        enrollment_digest,
                    } => crate::learning_automation::run_cycle(
                        paths.clone(),
                        profile.clone(),
                        vault_root.clone(),
                        enrollment_digest.clone(),
                    )
                    .await
                    .map(|v| harness_result(v, &spec.budget))
                    .map_err(|e| e.to_string()),
                    ScheduleTargetV2::Monitor { request } => {
                        crate::monitor_automation::run(request.clone(), &spec.context).await
                    }
                    _ => {
                        let started = chrono::Utc::now();
                        let execution = crate::schedule::execute_automation_target_inner(
                            paths,
                            profile,
                            &spec.definition,
                            uuid::Uuid::parse_str(&spec.context.run_id)
                                .map_err(|e| e.to_string())?,
                            spec.history.clone(),
                        )
                        .await?;
                        let (usage, authoritative) = crate::automation::execution_usage(
                            paths,
                            profile,
                            &spec.definition,
                            &execution,
                            started,
                            chrono::Utc::now(),
                        )
                        .map_err(|e| e.to_string())?;
                        Ok(NativeResult {
                            content: execution.content,
                            usage: if authoritative {
                                usage
                            } else {
                                AutomationUsageCountersV1::from_budget(&spec.budget)
                            },
                        })
                    }
                }
            }),
        )
        .await;
    match result {
        Ok(result) => {
            let v = serde_json::to_value(&result).map_err(|e| e.to_string())?;
            store
                .confirm_action("target", v.clone())
                .map_err(|e| e.to_string())?;
            let repository_attention =
                matches!(
                    spec.schedule.target,
                    ScheduleTargetV2::RepositoryTask { .. }
                ) && serde_json::from_str::<serde_json::Value>(&result.content)
                    .ok()
                    .and_then(|o| o.get("needs_attention").and_then(|v| v.as_bool()))
                    .unwrap_or(false);
            store
                .finish(
                    if repository_attention {
                        ExecutionState::NeedsAttention
                    } else {
                        ExecutionState::Succeeded
                    },
                    Some(v),
                    repository_attention
                        .then(|| "Verified candidate retained; integration needs review".into()),
                )
                .map_err(|e| e.to_string())
        }
        Err(error) => store
            .finish(ExecutionState::NeedsAttention, None, Some(error))
            .map_err(|e| e.to_string()),
    }
}
fn model_free_result(value: serde_json::Value) -> NativeResult {
    let content = serde_json::to_string_pretty(&value).unwrap_or_default();
    NativeResult {
        usage: AutomationUsageCountersV1 {
            tool_calls: 1,
            output_bytes: content.len() as u64,
            ..Default::default()
        },
        content,
    }
}
fn harness_result(value: serde_json::Value, budget: &AutomationBudgetV1) -> NativeResult {
    let mut usage: AutomationUsageCountersV1 = value
        .get("usage")
        .cloned()
        .and_then(|u| serde_json::from_value(u).ok())
        .unwrap_or_else(|| AutomationUsageCountersV1::from_budget(budget));
    // The frozen harness ledger reports requests/cost; unavailable token counts
    // retain their reservation rather than being interpreted as free usage.
    if usage.provider_requests > 0 && usage.input_tokens == 0 && usage.output_tokens == 0 {
        usage.input_tokens = budget.input_tokens;
        usage.output_tokens = budget.output_tokens;
    }
    let status = value
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("needs_review");
    let report = serde_json::json!({"schema":"ca.heartbeat-report.v1","status":match status{"unchanged"=>"unchanged","changed"=>"changed","completed"=>"completed","failed"=>"failed",_=>"needs_attention"},"summary":value.get("summary").cloned().unwrap_or_else(||serde_json::json!("Evaluation needs review")),"state":value.get("cycles").map(|v|v.to_string()).unwrap_or_else(||value.to_string())});
    let mut result = model_free_result(report);
    usage.output_bytes = result.content.len() as u64;
    result.usage = usage;
    result
}
pub fn inspect_run(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    run_id: &str,
    reconcile: bool,
    cancel: bool,
) -> Result<ExecutionReceipt, String> {
    let context =
        RunStore::load_context(&state_root(paths, profile), run_id).map_err(|e| e.to_string())?;
    let store = RunStore::open(&context).map_err(|e| e.to_string())?;
    if cancel {
        store.cancel().map_err(|e| e.to_string())?;
    }
    if reconcile {
        store.reconcile()
    } else {
        store.status()
    }
    .map_err(|e| e.to_string())
}

/// UI polling only handles completed messages. Filesystem admission and every
/// execution operation run on a separate task or owned child process.
pub(crate) struct Supervisor {
    last_activity: std::time::Instant,
    task: Option<tokio::task::JoinHandle<Result<usize, String>>>,
    next_scan: std::time::Instant,
    scope: Option<(RuntimePaths, ActiveProfile)>,
    last_notice: Option<String>,
}
impl Supervisor {
    pub fn new() -> Self {
        let now = std::time::Instant::now();
        Self {
            last_activity: now,
            task: None,
            next_scan: now,
            scope: None,
            last_notice: None,
        }
    }
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }
    pub fn user_activity(&mut self) {
        self.last_activity = std::time::Instant::now();
    }
    pub async fn poll(
        &mut self,
        idle: bool,
        _config: &crate::config::CaConfig,
        paths: &RuntimePaths,
        profile: &ActiveProfile,
    ) -> Option<String> {
        if self.task.as_ref().is_some_and(|task| task.is_finished()) {
            let result = self.task.take().expect("completed task").await;
            self.next_scan = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let notice = match result {
                Ok(Ok(_)) => None,
                Ok(Err(error)) => Some(format!("Automation needs attention: {error}")),
                Err(_) => {
                    Some("Automation supervision stopped; inspect durable run history".into())
                }
            };
            let changed = notice != self.last_notice;
            self.last_notice = notice.clone();
            return if changed { notice } else { None };
        }
        let now = std::time::Instant::now();
        if !idle {
            self.last_activity = now;
        }
        if self.task.is_some()
            || now < self.next_scan
            || !idle
            || now.saturating_duration_since(self.last_activity)
                < std::time::Duration::from_secs(300)
        {
            return None;
        }
        self.next_scan = now + std::time::Duration::from_secs(30);
        let (paths, profile) = (paths.clone(), profile.clone());
        self.scope = Some((paths.clone(), profile.clone()));
        self.task = Some(tokio::spawn(async move {
            crate::schedule::run_unified_quiet(&paths, &profile).await
        }));
        None
    }
    pub async fn shutdown(&mut self) {
        self.user_activity();
        // Stop only this supervisor's task. Its worker guard owns descendant
        // cleanup; runs admitted by another process retain their ownership.
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
        self.scope = None;
    }
}

#[cfg(all(test, unix))]
mod worker_failure_tests {
    use super::*;

    fn script_definition(
        workspace: &std::path::Path,
        script: PathBuf,
        args: Vec<String>,
    ) -> AutomationDefinitionV1 {
        AutomationDefinitionV1 {
            schema: crate::automation::AUTOMATION_SCHEMA.into(),
            id: "frozen-sandbox".into(),
            name: "Frozen sandbox fixture".into(),
            automation_kind: crate::automation::AutomationKindV1::Standalone,
            schedule: ScheduleDefinitionV2 {
                version: 2,
                id: "frozen-sandbox".into(),
                provider: None,
                model: None,
                workspace: workspace.to_path_buf(),
                runtime_secs: 30,
                output_bytes: 8192,
                tools: Vec::new(),
                grants: Vec::new(),
                allow_network: false,
                skills: Vec::new(),
                workspace_policy: crate::schedule_v2::WorkspacePolicy::ExistingReadOnly,
                pinned_revision: None,
                trigger: crate::schedule_v2::ScheduleTriggerV2::Delay { seconds: 1 },
                missed_run_policy: crate::schedule_v2::MissedRunPolicy::Skip,
                target: ScheduleTargetV2::ScriptOnly {
                    executable_sha256: format!(
                        "{:x}",
                        Sha256::digest(std::fs::read(&script).unwrap())
                    ),
                    executable: script,
                    args,
                    scrub_environment: true,
                },
                delivery: crate::schedule_v2::ScheduleDeliveryV2::None,
            },
            reasoning_effort: None,
            pricing: crate::automation::PricingBindingV1::ModelFree,
            context_sha256: Vec::new(),
            per_run_budget: AutomationBudgetV1::per_run_default(),
            rolling_30_day_budget: AutomationBudgetV1::rolling_default(),
            notification_policy: crate::automation::AutomationNotificationPolicy::AllResults,
        }
    }

    #[test]
    fn native_sandbox_policy_binding_detects_changes_without_changing_v1_definition_identity() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let script = root.join("fixture.sh");
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        let definition = script_definition(&root, script, Vec::new());
        let paths = RuntimePaths::from_home(root);
        let run_id = Uuid::new_v4();
        let required = execution_context_with_mode(
            &paths,
            &ActiveProfile::Default,
            &definition,
            run_id,
            crate::sandbox::SandboxMode::Required,
        )
        .unwrap();
        let off = execution_context_with_mode(
            &paths,
            &ActiveProfile::Default,
            &definition,
            run_id,
            crate::sandbox::SandboxMode::Off,
        )
        .unwrap();
        assert_eq!(required.definition_digest, off.definition_digest);
        assert_eq!(
            required.resource_binding_digest,
            off.resource_binding_digest
        );
        assert_ne!(required.policy_digest, off.policy_digest);
        let binding = sandbox_binding(&definition, crate::sandbox::SandboxMode::Required).unwrap();
        assert_eq!(
            validate_sandbox_binding(&required, &binding).unwrap(),
            crate::sandbox::SandboxMode::Required
        );
        assert!(validate_sandbox_binding(&off, &binding).is_err());
        assert_eq!(
            validate_sandbox_binding(
                &off,
                &sandbox_binding(&definition, crate::sandbox::SandboxMode::Off).unwrap()
            )
            .unwrap(),
            crate::sandbox::SandboxMode::Off
        );
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn native_frozen_sandbox_child_fixture() {
        if std::env::var_os("CA_AUTOMATION_FROZEN_SANDBOX_FIXTURE").is_none() {
            return;
        }
        let paths = RuntimePaths::discover().unwrap();
        run_stdio(&paths, &ActiveProfile::Default).await.unwrap();
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn native_worker_keeps_required_command_sandbox_after_profile_is_changed_to_off() {
        use std::os::unix::fs::PermissionsExt;
        use std::process::Stdio;
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let workspace = root.join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let marker = root.join("unapproved-write");
        let script = workspace.join("fixture.sh");
        std::fs::write(&script, "#!/bin/sh\nif printf unapproved >\"$1\"; then exit 9; fi\nprintf 'required sandbox retained'\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let definition = script_definition(&workspace, script, vec![marker.display().to_string()]);
        let paths = RuntimePaths::from_home(root.clone());
        std::fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
        std::fs::write(&paths.config_file, "[agent]\nsandbox_mode = \"required\"\n").unwrap();
        let context =
            execution_context(&paths, &ActiveProfile::Default, &definition, Uuid::new_v4())
                .unwrap();
        let store = RunStore::open(&context).unwrap();
        let resources = store.acquire_resources().unwrap();
        let spec = NativeWorkerSpec {
            context,
            schedule: definition.schedule.clone(),
            budget: definition.per_run_budget.clone(),
            workflow_config: None,
            sandbox_binding: sandbox_binding(&definition, crate::sandbox::SandboxMode::Required)
                .unwrap(),
            definition,
            history: Vec::new(),
        };
        std::fs::write(&paths.config_file, "[agent]\nsandbox_mode = \"off\"\n").unwrap();
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "automation_runtime::worker_failure_tests::native_frozen_sandbox_child_fixture",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", &root)
            .env("PATH", "/usr/bin:/bin")
            .env("CA_AUTOMATION_FROZEN_SANDBOX_FIXTURE", "1")
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        inherit_resource_leases(&mut command, &resources);
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&spec).unwrap())
            .await
            .unwrap();
        let output =
            tokio::time::timeout(std::time::Duration::from_secs(60), child.wait_with_output())
                .await
                .unwrap()
                .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt = store.status().unwrap();
        assert_eq!(
            receipt.state,
            ExecutionState::Succeeded,
            "{:?}",
            receipt.reason
        );
        assert!(
            !marker.exists(),
            "profile reload widened the admitted physical command"
        );
        assert!(result_from_receipt(receipt)
            .unwrap()
            .content
            .contains("required sandbox retained"));
    }

    fn confirmed_target() -> (tempfile::TempDir, RunStore, NativeResult) {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let context = ExecutionContext {
            schema: EXECUTION_SCHEMA.into(),
            run_id: "00000000-0000-4000-8000-000000000093".into(),
            profile: "default".into(),
            state_root: root.join("state"),
            lease_root: root.join("leases"),
            definition_digest: "a".repeat(64),
            resource_binding_digest: "b".repeat(64),
            policy_digest: "c".repeat(64),
            resources: Vec::new(),
            runtime_seconds: 30,
            output_bytes: 4096,
        };
        let store = RunStore::open(&context).unwrap();
        store.start().unwrap();
        store.prepare_action("target").unwrap();
        assert!(store.begin_action("target").unwrap());
        let result = NativeResult {
            content: "Confirmed effect retained".into(),
            usage: AutomationUsageCountersV1::default(),
        };
        store
            .confirm_action("target", serde_json::to_value(&result).unwrap())
            .unwrap();
        (temporary, store, result)
    }

    fn assert_recovery_without_replay(store: &RunStore, result: &NativeResult) {
        let recovered = store.reconcile().unwrap();
        assert_eq!(recovered.state, ExecutionState::Succeeded);
        assert_eq!(
            result_from_receipt(recovered.clone()).unwrap().content,
            result.content
        );
        assert_eq!(store.start().unwrap(), recovered);
        assert!(store.prepare_action("target").is_err());
        assert!(store.begin_action("target").is_err());
    }

    #[tokio::test]
    async fn nonzero_native_child_receipt_is_rejected_and_confirmed_effect_recovers_once() {
        let (temporary, store, result) = confirmed_target();
        let receipt = store.status().unwrap();
        let output_path = temporary.path().join("worker-output.json");
        std::fs::write(&output_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        let output = tokio::process::Command::new("/bin/sh")
            .args(["-c", "cat \"$1\"; exit 2", "native-child-fixture"])
            .arg(output_path)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .output()
            .await
            .unwrap();
        assert!(!output.status.success());
        assert_eq!(
            serde_json::from_slice::<ExecutionReceipt>(&output.stdout).unwrap(),
            receipt
        );
        assert!(crate::automation_engine_cli::validate_worker_receipt(
            &output.status,
            &output.stdout,
            &receipt,
        )
        .is_err());
        assert_recovery_without_replay(&store, &result);
    }

    #[tokio::test]
    async fn malformed_native_child_output_does_not_repeat_a_confirmed_effect() {
        let (_temporary, store, result) = confirmed_target();
        let receipt = store.status().unwrap();
        let output = tokio::process::Command::new("/bin/sh")
            .args(["-c", "printf 'fixture-private-malformed-output'"])
            .env_clear()
            .output()
            .await
            .unwrap();
        assert!(output.status.success());
        let error = crate::automation_engine_cli::validate_worker_receipt(
            &output.status,
            &output.stdout,
            &receipt,
        )
        .unwrap_err();
        assert!(!error.contains("fixture-private-malformed-output"));
        assert_recovery_without_replay(&store, &result);
    }
}
#[test]
fn learning_physical_accounting_distinguishes_absence_known_usage_and_unknown_attempt() {
    use codex_team::self_improvement::{LearningCallAccounting, UsageV1};
    let budget = AutomationBudgetV1::per_run_default();
    let usage_result = |accounting| {
        harness_result(
            serde_json::json!({"status":"unchanged","usage":crate::learning_automation::stage_usage(accounting)}),
            &budget,
        ).usage
    };
    let absent = usage_result(None);
    assert_eq!(absent.provider_requests, 0);
    assert_eq!((absent.input_tokens, absent.output_tokens), (0, 0));
    assert_eq!(absent.cost_usd, crate::usage::DecimalUsd::ZERO);
    let known = usage_result(Some(&LearningCallAccounting {
        provider_requests: Some(1),
        reserved_provider_requests: 0,
        usage: Some(UsageV1 {
            input_tokens: 7,
            output_tokens: 2,
            estimated_microusd: 1_234,
        }),
        reserved_microusd: 0,
    }));
    assert_eq!(known.provider_requests, 1);
    assert_eq!((known.input_tokens, known.output_tokens), (7, 2));
    assert_eq!(known.cost_usd, "0.001234".parse().unwrap());
    let unknown = usage_result(Some(&LearningCallAccounting {
        provider_requests: None,
        reserved_provider_requests: 1,
        usage: None,
        reserved_microusd: 1_234,
    }));
    assert_eq!(unknown.provider_requests, budget.provider_requests);
    assert_eq!(
        (unknown.input_tokens, unknown.output_tokens),
        (budget.input_tokens, budget.output_tokens)
    );
    assert_eq!(unknown.cost_usd, budget.max_cost_usd);
}
