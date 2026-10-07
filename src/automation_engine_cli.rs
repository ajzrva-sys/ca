//! Provider-free host facade for the absorbed typed engine.
use ca_automation::native::{AutomationEngine, EngineOperation};
use ca_automation::runtime::{ExecutionContext, ExecutionReceipt, RunStore, EXECUTION_SCHEMA};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Parser)]
#[command(
    name = "ca automation-engine",
    about = "Preview and operate one durable typed automation without a provider"
)]
pub struct EngineCli {
    #[arg(long)]
    profile: Option<String>,
    #[command(subcommand)]
    command: EngineCommand,
}

#[derive(Debug, Subcommand)]
enum EngineCommand {
    Preview(InvocationArgs),
    Start {
        #[command(flatten)]
        invocation: InvocationArgs,
        /// Exact digest returned by preview for this occurrence and authority
        #[arg(long)]
        confirm: String,
    },
    Status {
        run_id: String,
    },
    Cancel {
        run_id: String,
    },
    Reconcile {
        run_id: String,
    },
    Doctor {
        #[arg(long)]
        repository: Option<PathBuf>,
        #[arg(long)]
        delegate_profile: Option<PathBuf>,
    },
    ConfigMigration {
        #[command(subcommand)]
        command: MigrationCommand,
    },
    #[command(hide = true, name = "__worker")]
    Worker,
    #[command(hide = true, name = "__compatibility-workflow")]
    CompatibilityWorkflow,
}

#[derive(Debug, clap::Args)]
struct InvocationArgs {
    /// JSON EngineOperation, retaining the existing V1 workflow/request schemas
    input: PathBuf,
    /// Explicit UUID: reuse it to reconcile; use a new UUID for a new occurrence
    #[arg(long)]
    run_id: String,
    #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..=86_400))]
    runtime_seconds: u64,
    #[arg(long, default_value_t = 1_048_576, value_parser = clap::value_parser!(u64).range(1..=8_388_608))]
    output_bytes: u64,
    #[arg(long)]
    allow_network: bool,
    #[arg(long, value_enum, default_value_t = crate::sandbox::SandboxMode::Required)]
    sandbox: crate::sandbox::SandboxMode,
    /// Pass only explicitly named credential environment variables to the worker
    #[arg(long = "credential-env")]
    credential_env: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum MigrationCommand {
    Preview {
        #[arg(long)]
        source: PathBuf,
    },
    Apply {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        confirm: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Invocation {
    context: ExecutionContext,
    operation: EngineOperation,
    workspace: PathBuf,
    allow_network: bool,
    sandbox: String,
    credential_env: Vec<String>,
    confirmation_digest: String,
    #[serde(default, skip_serializing_if = "is_false")]
    compatibility: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

pub fn parse_args<I, T>(args: I) -> Result<EngineCli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    EngineCli::try_parse_from(args)
}

pub async fn run_cli(cli: EngineCli) -> i32 {
    match run_inner(cli).await {
        Ok(value) => {
            println!("{value}");
            0
        }
        Err(error) => {
            eprintln!("ca automation-engine: {error}");
            2
        }
    }
}

async fn run_inner(cli: EngineCli) -> Result<serde_json::Value, String> {
    let profile = crate::runtime::ActiveProfile::parse(cli.profile.as_deref())
        .map_err(|error| error.to_string())?;
    if profile == crate::runtime::ActiveProfile::Bare {
        return Err("Automation execution requires a persistent profile".into());
    }
    let paths = crate::runtime::RuntimePaths::discover().map_err(|error| error.to_string())?;
    match cli.command {
        EngineCommand::Preview(args) => {
            let invocation = load_invocation(&args, &paths, &profile)?;
            preview(&invocation)
        }
        EngineCommand::Start {
            invocation: args,
            confirm,
        } => {
            let invocation = load_invocation(&args, &paths, &profile)?;
            if confirm != invocation.confirmation_digest {
                return Err(format!(
                    "Exact preview digest required: {}",
                    invocation.confirmation_digest
                ));
            }
            serde_json::to_value(start(&invocation, &paths, &profile).await?)
                .map_err(|error| error.to_string())
        }
        EngineCommand::Status { run_id } => manage(&paths, &profile, &run_id, "status"),
        EngineCommand::Cancel { run_id } => manage(&paths, &profile, &run_id, "cancel"),
        EngineCommand::Reconcile { run_id } => manage(&paths, &profile, &run_id, "reconcile"),
        EngineCommand::Doctor {
            repository,
            delegate_profile,
        } => {
            let repository = repository.or_else(|| std::env::current_dir().ok());
            let default_profile = paths.config_root.join("profiles/codex-team-delegate.toml");
            let delegate_profile =
                delegate_profile.or_else(|| default_profile.exists().then_some(default_profile));
            serde_json::to_value(
                crate::automation_engine_migration::doctor(
                    repository.as_deref(),
                    delegate_profile.as_deref(),
                )
                .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())
        }
        EngineCommand::ConfigMigration { command } => {
            let destination = paths
                .profile_data_dir(&profile)
                .join("automation-imports/v1");
            let apply = matches!(&command, MigrationCommand::Apply { .. });
            let result = match command {
                MigrationCommand::Preview { source } => {
                    crate::automation_engine_migration::preview(&source, &destination)
                }
                MigrationCommand::Apply { source, confirm } => {
                    crate::automation_engine_migration::apply(&source, &destination, &confirm)
                }
            }
            .map_err(|error| error.to_string())?;
            let installed_records = if apply {
                crate::automation_engine_migration::install_workflow_records(
                    &result,
                    &crate::automation_runtime::state_root(&paths, &profile),
                    &crate::automation_runtime::lease_root(&paths),
                )
                .map_err(|error| error.to_string())?
            } else {
                0
            };
            let mut result = serde_json::to_value(result).map_err(|error| error.to_string())?;
            result
                .as_object_mut()
                .ok_or("Migration preview was not an object")?
                .insert("installed_records".into(), installed_records.into());
            Ok(result)
        }
        EngineCommand::Worker => run_worker(&paths, &profile).await,
        EngineCommand::CompatibilityWorkflow => {
            let mut bytes = Vec::new();
            std::io::stdin()
                .take(512 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            if bytes.len() > 512 * 1024 {
                return Err("Compatibility envelope exceeds its bound".into());
            }
            let envelope: ca_automation::automation_workflow::CompatibilityWorkflowInvocation =
                serde_json::from_slice(&bytes).map_err(|_| "Compatibility envelope is invalid")?;
            let invocation = compatibility_invocation(envelope)?;
            serde_json::to_value(start(&invocation, &paths, &profile).await?)
                .map_err(|error| error.to_string())
        }
    }
}

fn manage(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    run_id: &str,
    command: &str,
) -> Result<serde_json::Value, String> {
    let context = RunStore::load_context(
        &crate::automation_runtime::state_root(paths, profile),
        run_id,
    )
    .map_err(|error| error.to_string())?;
    require_profile(&context, paths, profile)?;
    let receipt = match command {
        "status" => AutomationEngine::status(&context),
        "cancel" => AutomationEngine::cancel(&context),
        _ => AutomationEngine::reconcile(&context),
    }
    .map_err(|error| error.to_string())?;
    serde_json::to_value(receipt).map_err(|error| error.to_string())
}

fn digest(value: &impl Serialize) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|error| error.to_string())?)
    ))
}

fn load_invocation(
    args: &InvocationArgs,
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
) -> Result<Invocation, String> {
    uuid::Uuid::parse_str(&args.run_id).map_err(|_| "Execution run ID must be an explicit UUID")?;
    let file = std::fs::File::open(&args.input).map_err(|error| error.to_string())?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Engine input must be a regular JSON file".into());
    }
    let mut bytes = Vec::new();
    file.take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 256 * 1024 {
        return Err("Engine operation exceeds its input bound".into());
    }
    let operation: EngineOperation =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    make_invocation(operation, args, paths, profile)
}

fn make_invocation(
    operation: EngineOperation,
    args: &InvocationArgs,
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
) -> Result<Invocation, String> {
    let operation = match operation {
        EngineOperation::Workflow {
            config_path,
            request,
        } => EngineOperation::Workflow {
            config_path: config_path
                .canonicalize()
                .map_err(|error| error.to_string())?,
            request,
        },
        EngineOperation::Mechanistic {
            repository,
            request,
        } => EngineOperation::Mechanistic {
            repository: repository.canonicalize().map_err(|_| {
                "Repository binding is unavailable; repair its path before execution"
            })?,
            request,
        },
    };
    let mut credentials = args.credential_env.clone();
    credentials.sort();
    credentials.dedup();
    validate_credential_env(&credentials)?;
    let (workspace, resource_binding, resources) = match &operation {
        EngineOperation::Workflow {
            config_path,
            request,
        } => {
            let canonical = config_path
                .canonicalize()
                .map_err(|error| error.to_string())?;
            let file = std::fs::File::open(&canonical).map_err(|error| error.to_string())?;
            let mut bytes = Vec::new();
            file.take(64 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            if bytes.len() > 64 * 1024 {
                return Err("Workflow configuration exceeds its input bound".into());
            }
            let roots = AutomationEngine::workflow_local_roots(&canonical)
                .map_err(|error| error.to_string())?;
            if !args.allow_network
                && matches!(
                    &request.operation,
                    ca_automation_protocol::WorkflowOperationV1::Run { .. }
                )
            {
                return Err(
                    "Workflow execution requires its explicit --allow-network grant".into(),
                );
            }
            (
                canonical
                    .parent()
                    .ok_or("Workflow configuration has no parent")?
                    .to_path_buf(),
                digest(&(
                    canonical.clone(),
                    format!("{:x}", Sha256::digest(bytes)),
                    roots,
                ))?,
                if matches!(
                    &request.operation,
                    ca_automation_protocol::WorkflowOperationV1::Run { .. }
                ) {
                    vec![
                        format!("workflow:{}", canonical.display()),
                        "typed-workflow-mutation".into(),
                    ]
                } else {
                    vec![format!("workflow:{}", canonical.display())]
                },
            )
        }
        EngineOperation::Mechanistic {
            repository,
            request,
        } => {
            let canonical = repository.canonicalize().map_err(|_| {
                "Repository binding is unavailable; repair its path before execution"
            })?;
            let revision = AutomationEngine::repository_revision(&canonical)
                .map_err(|error| error.to_string())?;
            if request.expected_revision != revision {
                return Err("Mechanistic request revision is stale; refresh the typed request before preview".into());
            }
            (
                canonical.clone(),
                digest(&(canonical.clone(), revision))?,
                vec![format!("repository:{}", canonical.display())],
            )
        }
    };
    let context = ExecutionContext {
        schema: EXECUTION_SCHEMA.into(),
        run_id: args.run_id.clone(),
        profile: profile.label().into(),
        state_root: crate::automation_runtime::state_root(paths, profile),
        lease_root: crate::automation_runtime::lease_root(paths),
        definition_digest: digest(&operation)?,
        resource_binding_digest: resource_binding,
        policy_digest: digest(&(
            args.runtime_seconds,
            args.output_bytes,
            args.allow_network,
            args.sandbox.as_str(),
            &credentials,
        ))?,
        resources,
        runtime_seconds: args.runtime_seconds,
        output_bytes: args.output_bytes,
    };
    let confirmation_digest = digest(&context)?;
    let invocation = Invocation {
        context,
        operation,
        workspace,
        allow_network: args.allow_network,
        sandbox: args.sandbox.as_str().into(),
        credential_env: credentials,
        confirmation_digest,
        compatibility: false,
    };
    AutomationEngine::preview(&invocation.context, &invocation.operation)
        .map_err(|error| error.to_string())?;
    Ok(invocation)
}

fn validate_credential_env(credentials: &[String]) -> Result<(), String> {
    if credentials.len() > 32
        || credentials.iter().any(|key| {
            key.is_empty()
                || key.len() > 128
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || key.as_bytes()[0].is_ascii_digit()
                || key.starts_with("DYLD_")
                || key.starts_with("LD_")
                || key.starts_with("GIT_")
                || matches!(
                    key.as_str(),
                    "HOME"
                        | "PATH"
                        | "TMPDIR"
                        | "ENV"
                        | "BASH_ENV"
                        | "ZDOTDIR"
                        | "SHELL"
                        | "RUSTUP_HOME"
                        | "CARGO_HOME"
                )
        })
    {
        return Err("Credential environment names must be bounded identifiers".into());
    }
    Ok(())
}

fn compatibility_invocation(
    envelope: ca_automation::automation_workflow::CompatibilityWorkflowInvocation,
) -> Result<Invocation, String> {
    let rebound = ca_automation::automation_workflow::compatibility_invocation(
        &envelope.config_path,
        &envelope.context.state_root,
        envelope.request.clone(),
        Some(envelope.context.run_id.clone()),
    )
    .map_err(|error| error.to_string())?;
    if envelope.context != rebound.context || envelope.config_path != rebound.config_path {
        return Err("Compatibility authority changed before ca admission".into());
    }
    let credential_env = AutomationEngine::workflow_credential_env(&envelope.config_path)
        .map_err(|error| error.to_string())?;
    validate_credential_env(&credential_env)?;
    let workspace = envelope
        .config_path
        .parent()
        .ok_or("Workflow configuration has no parent")?
        .to_path_buf();
    let confirmation_digest = digest(&envelope.context)?;
    Ok(Invocation {
        context: envelope.context,
        operation: EngineOperation::Workflow {
            config_path: envelope.config_path,
            request: envelope.request,
        },
        workspace,
        allow_network: true,
        sandbox: crate::sandbox::SandboxMode::Required.as_str().into(),
        credential_env,
        confirmation_digest,
        compatibility: true,
    })
}

fn rebind_compatibility(invocation: &Invocation) -> Result<Invocation, String> {
    let EngineOperation::Workflow {
        config_path,
        request,
    } = &invocation.operation
    else {
        return Err("Compatibility dispatch requires a typed workflow".into());
    };
    let rebound = compatibility_invocation(
        ca_automation::automation_workflow::CompatibilityWorkflowInvocation {
            context: invocation.context.clone(),
            config_path: config_path.clone(),
            request: request.clone(),
        },
    )?;
    if !invocation.allow_network
        || invocation.sandbox != rebound.sandbox
        || invocation.credential_env != rebound.credential_env
    {
        return Err("Compatibility execution policy changed".into());
    }
    Ok(rebound)
}

fn preview(invocation: &Invocation) -> Result<serde_json::Value, String> {
    let mut value = AutomationEngine::preview(&invocation.context, &invocation.operation)
        .map_err(|error| error.to_string())?;
    value["confirmation_digest"] = invocation.confirmation_digest.clone().into();
    value["runtime_seconds"] = invocation.context.runtime_seconds.into();
    value["output_bytes"] = invocation.context.output_bytes.into();
    value["profile"] = invocation.context.profile.clone().into();
    Ok(value)
}

fn require_profile(
    context: &ExecutionContext,
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
) -> Result<(), String> {
    context.validate()?;
    if context.profile != profile.label()
        || context.state_root != crate::automation_runtime::state_root(paths, profile)
        || context.lease_root != crate::automation_runtime::lease_root(paths)
    {
        return Err("Execution context does not belong to the selected ca profile".into());
    }
    Ok(())
}

async fn run_worker(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
) -> Result<serde_json::Value, String> {
    crate::jobs::enable_worker_containment()?;
    ca_automation::runtime::enable_worker_containment().map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(512 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 512 * 1024 {
        return Err("Engine worker envelope exceeds its bound".into());
    }
    let invocation: Invocation =
        serde_json::from_slice(&bytes).map_err(|_| "Engine worker envelope is invalid")?;
    if !invocation.compatibility {
        require_profile(&invocation.context, paths, profile)?;
    }
    let _inherited_resources =
        crate::automation_runtime::adopt_resource_leases(&invocation.context)?;
    let args = InvocationArgs {
        input: PathBuf::new(),
        run_id: invocation.context.run_id.clone(),
        runtime_seconds: invocation.context.runtime_seconds,
        output_bytes: invocation.context.output_bytes,
        allow_network: invocation.allow_network,
        sandbox: crate::sandbox::SandboxMode::from_saved_value(&invocation.sandbox)
            .map_err(|error| error.to_string())?,
        credential_env: invocation.credential_env.clone(),
    };
    let rebound = if invocation.compatibility {
        rebind_compatibility(&invocation)?
    } else {
        make_invocation(invocation.operation.clone(), &args, paths, profile)?
    };
    let changed = changed_binding_fields(&invocation, &rebound);
    if !changed.is_empty() {
        return Err(format!(
            "Engine bindings changed between admission and execution ({})",
            changed.join(", ")
        ));
    }
    let receipt = AutomationEngine::start(&invocation.context, invocation.operation)
        .await
        .map_err(|error| error.to_string())?;
    serde_json::to_value(receipt).map_err(|error| error.to_string())
}

fn changed_binding_fields(expected: &Invocation, actual: &Invocation) -> Vec<&'static str> {
    let expected_context = &expected.context;
    let actual_context = &actual.context;
    [
        ("schema", expected_context.schema != actual_context.schema),
        ("run_id", expected_context.run_id != actual_context.run_id),
        (
            "profile",
            expected_context.profile != actual_context.profile,
        ),
        (
            "state_root",
            expected_context.state_root.as_os_str() != actual_context.state_root.as_os_str(),
        ),
        (
            "lease_root",
            expected_context.lease_root.as_os_str() != actual_context.lease_root.as_os_str(),
        ),
        (
            "definition_digest",
            expected_context.definition_digest != actual_context.definition_digest,
        ),
        (
            "resource_binding_digest",
            expected_context.resource_binding_digest != actual_context.resource_binding_digest,
        ),
        (
            "policy_digest",
            expected_context.policy_digest != actual_context.policy_digest,
        ),
        (
            "resources",
            expected_context.resources != actual_context.resources,
        ),
        (
            "runtime_seconds",
            expected_context.runtime_seconds != actual_context.runtime_seconds,
        ),
        (
            "output_bytes",
            expected_context.output_bytes != actual_context.output_bytes,
        ),
        (
            "confirmation_digest",
            expected.confirmation_digest != actual.confirmation_digest,
        ),
        (
            "workspace",
            expected.workspace.as_os_str() != actual.workspace.as_os_str(),
        ),
        (
            "compatibility",
            expected.compatibility != actual.compatibility,
        ),
    ]
    .into_iter()
    .filter_map(|(name, changed)| changed.then_some(name))
    .collect()
}

async fn start(
    invocation: &Invocation,
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
) -> Result<ExecutionReceipt, String> {
    let store = RunStore::open(&invocation.context).map_err(|error| error.to_string())?;
    let receipt = store.reconcile().map_err(|error| error.to_string())?;
    if receipt.state.terminal() {
        return Ok(receipt);
    }
    // Parent ownership survives worker exit until every owned descendant and
    // pipe reader has drained. The verified worker inherits this admission.
    let workflow_run = matches!(&invocation.operation, EngineOperation::Workflow { request, .. }
        if matches!(&request.operation, ca_automation_protocol::WorkflowOperationV1::Run { .. }));
    let workflow_admission = if workflow_run {
        match ca_automation::automation_workflow::admit_workflow_run(&store, false)
            .await
            .map_err(|error| error.to_string())?
        {
            Some(admission) => Some(admission),
            None => return store.status().map_err(|error| error.to_string()),
        }
    } else {
        None
    };
    let direct_resources = if workflow_run {
        Vec::new()
    } else {
        store
            .acquire_resources()
            .map_err(|error| error.to_string())?
    };
    let resources = workflow_admission
        .as_ref()
        .map_or(&direct_resources, |admission| &admission.resources);
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
    let mut roots = vec![
        invocation.context.state_root.clone(),
        invocation.context.lease_root.clone(),
    ];
    let mut read_only_roots = vec![
        invocation.workspace.clone(),
        paths.config_root.clone(),
        executable.clone(),
    ];
    match &invocation.operation {
        EngineOperation::Workflow { config_path, .. } => {
            if invocation.allow_network {
                roots.extend(
                    AutomationEngine::workflow_local_roots(config_path)
                        .map_err(|error| error.to_string())?,
                );
                read_only_roots.push(paths.home.join(".ssh"));
            }
        }
        EngineOperation::Mechanistic {
            repository,
            request,
        } => {
            if !request.actions.iter().all(|action| action.is_query()) {
                roots.push(repository.clone());
            }
        }
    }
    let arguments = vec![
        "automation-engine".into(),
        "--profile".into(),
        profile.label().into(),
        "__worker".into(),
    ];
    let mode = crate::sandbox::SandboxMode::from_saved_value(&invocation.sandbox)
        .map_err(|error| error.to_string())?;
    let command_isolation = matches!(
        &invocation.operation,
        EngineOperation::Mechanistic { request, .. }
            if AutomationEngine::requires_command_isolation(&request.actions)
    );
    let prepared = crate::sandbox::SandboxBackend::new(if command_isolation {
        // The trusted coordinator retains profile state; each physical
        // evidence command applies its own required private projection.
        crate::sandbox::SandboxMode::PolicyOnly
    } else {
        mode
    })
    .prepare_process(
        &executable,
        &arguments,
        &crate::sandbox::SandboxPolicy {
            roots,
            read_only_roots,
            temporary_directory: temporary.path().into(),
            network_allowed: invocation.allow_network,
            hide_other_processes: false,
        },
    )
    .map_err(|error| error.to_string())?;
    let admission = crate::schedule::admit_native_worker(&invocation.workspace).await?;
    let mut command = tokio::process::Command::new(prepared.program);
    command
        .args(prepared.arguments)
        .current_dir(&invocation.workspace)
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
    let mut credential_values = Vec::new();
    for key in &invocation.credential_env {
        if let Some(value) = std::env::var_os(key) {
            credential_values.push(value.to_string_lossy().into_owned());
            command.env(key, value);
        }
    }
    command.env("TMPDIR", temporary.path());
    #[cfg(unix)]
    command.process_group(0);
    crate::automation_runtime::inherit_resource_leases(&mut command, resources);
    let mut worker = admission.attach(command.spawn().map_err(|error| error.to_string())?);
    let mut stdin = worker
        .child_mut()
        .stdin
        .take()
        .ok_or("Engine worker input unavailable")?;
    let output = worker
        .child_mut()
        .stdout
        .take()
        .ok_or("Engine worker output unavailable")?;
    let errors = worker
        .child_mut()
        .stderr
        .take()
        .ok_or("Engine worker diagnostics unavailable")?;
    let bytes = serde_json::to_vec(invocation).map_err(|error| error.to_string())?;
    let input_task = tokio::spawn(async move {
        stdin.write_all(&bytes).await?;
        stdin.shutdown().await
    });
    let output_task = tokio::spawn(read_output(
        output,
        invocation.context.output_bytes as usize + 256 * 1024,
    ));
    let error_task = tokio::spawn(read_output(errors, 64 * 1024));
    let cancellation = async {
        loop {
            if store.cancel_requested().unwrap_or(true) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    };
    let exit = tokio::select! {
        status = worker.child_mut().wait() => status.map_err(|error| error.to_string()),
        _ = tokio::time::sleep(workflow_admission.as_ref().map_or(
            std::time::Duration::from_secs(invocation.context.runtime_seconds),
            |admission| admission.remaining_runtime(),
        )) => Err("Engine runtime budget exhausted".into()),
        _ = cancellation => Err("Engine occurrence cancelled".into()),
    };
    let cleanup = worker
        .retire(!exit.as_ref().is_ok_and(|status| status.success()))
        .await
        .map_err(|_| "Engine worker cleanup stopped")?;
    let _ = input_task.await;
    let output = output_task.await;
    let diagnostics = match error_task.await {
        Ok(Ok(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
        Ok(Err(error)) => format!("Worker diagnostics unavailable: {error}"),
        Err(_) => "Worker diagnostics reader stopped".into(),
    };
    let failure = |reason: &str| worker_failure(reason, &diagnostics, &credential_values);
    let status = match exit {
        Ok(status) => status,
        Err(error) => {
            let _ = store.reconcile();
            return Err(failure(&error));
        }
    };
    if !status.success() {
        let _ = store.reconcile();
        return Err(failure(&format!("Engine worker failed ({status})")));
    }
    if cleanup {
        let _ = store.reconcile();
        return Err(failure(
            "Engine worker left commands active; its descendants were stopped",
        ));
    }
    let output = match output {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(error)) => {
            let _ = store.reconcile();
            return Err(failure(&error));
        }
        Err(_) => {
            let _ = store.reconcile();
            return Err(failure("Engine output reader stopped"));
        }
    };
    let persisted = store
        .status()
        .map_err(|error| failure(&error.to_string()))?;
    match validate_worker_receipt(&status, &output, &persisted) {
        Ok(receipt) => Ok(receipt),
        Err(error) => {
            // A confirmed durable outcome remains available through explicit
            // status/reconcile or duplicate dispatch. Abnormal transport is
            // never evidence that the effect may safely be repeated.
            let _ = store.reconcile();
            Err(failure(&error))
        }
    }
}

pub(crate) fn validate_worker_receipt(
    status: &std::process::ExitStatus,
    output: &[u8],
    persisted: &ExecutionReceipt,
) -> Result<ExecutionReceipt, String> {
    if !status.success() {
        return Err(format!("Automation worker failed ({status})"));
    }
    let receipt: ExecutionReceipt = serde_json::from_slice(output).map_err(|error| {
        format!(
            "Automation worker returned invalid output ({} bytes; {:?} at line {}, column {}); inspect its durable run",
            output.len(), error.classify(), error.line(), error.column()
        )
    })?;
    if &receipt != persisted {
        return Err("Automation worker output differs from its durable receipt".into());
    }
    Ok(receipt)
}

pub(crate) fn worker_failure(
    reason: &str,
    diagnostics: &str,
    credential_values: &[String],
) -> String {
    let secrets = credential_values
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let safe = crate::request::safe_text(diagnostics, &secrets);
    let safe = safe
        .split_inclusive(char::is_whitespace)
        .map(|token| {
            let bare = token.trim_end_matches(char::is_whitespace);
            let lowercase = bare.to_ascii_lowercase();
            if bare.contains("://")
                || ["sk-", "ghp_", "xoxb-", "AKIA"]
                    .iter()
                    .any(|prefix| bare.contains(prefix))
                || lowercase.starts_with("eyj") && bare.len() >= 32
            {
                format!("[REDACTED]{}", &token[bare.len()..])
            } else {
                token.to_owned()
            }
        })
        .collect::<String>();
    let safe = safe.chars().take(1024).collect::<String>();
    let reason = crate::request::safe_text(reason, &secrets)
        .chars()
        .take(1024)
        .collect::<String>();
    if safe.trim().is_empty() {
        format!("{reason}; worker diagnostics were empty")
    } else {
        format!("{reason}; worker diagnostics: {}", safe.trim())
    }
}

async fn read_output(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8192];
    let mut exceeded = false;
    loop {
        let count = reader
            .read(&mut chunk)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        if count > limit.saturating_sub(bytes.len()) {
            exceeded = true;
        } else if !exceeded {
            bytes.extend_from_slice(&chunk[..count]);
        }
    }
    if exceeded {
        Err("Engine worker output exceeds its admitted bound".into())
    } else {
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_binding_retains_v1_identity_and_refuses_policy_or_resource_changes() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let config = root.join("remote.toml");
        std::fs::write(&config,
            "schema='codex-team.remote_config.v1'\n[transmission.fixture]\nurl='http://127.0.0.1:9091/rpc'\nprotocol='legacy'\n").unwrap();
        std::fs::set_permissions(&config, {
            use std::os::unix::fs::PermissionsExt;
            std::fs::Permissions::from_mode(0o600)
        })
        .unwrap();
        let request = serde_json::from_value(serde_json::json!({
            "schema":"codex-team.workflow_request.v1", "operation":{"kind":"run", "workflow_id":"compatibility_fixture"}
        })).unwrap();
        let envelope = ca_automation::automation_workflow::compatibility_invocation(
            &config,
            &root.join("legacy-state"),
            request,
            Some("legacy_original_run".into()),
        )
        .unwrap();
        let original_context = envelope.context.clone();
        let mut invocation = compatibility_invocation(envelope.clone()).unwrap();
        assert_eq!(invocation.context, original_context);
        assert!(invocation.compatibility);
        assert_eq!(invocation.sandbox, "required");
        assert!(rebind_compatibility(&invocation).is_ok());
        invocation.sandbox = "off".into();
        assert!(rebind_compatibility(&invocation).is_err());
        std::fs::write(&config,
            "schema='codex-team.remote_config.v1'\n[transmission.fixture]\nurl='http://127.0.0.1:9092/rpc'\nprotocol='legacy'\n").unwrap();
        assert!(compatibility_invocation(envelope).is_err());
    }
    #[test]
    fn compatibility_credentials_cannot_override_process_loading_or_toolchain_policy() {
        assert!(validate_credential_env(&["SEEDBOX_PASSWORD".into()]).is_ok());
        for name in [
            "PATH",
            "DYLD_INSERT_LIBRARIES",
            "LD_PRELOAD",
            "GIT_CONFIG",
            "CARGO_HOME",
        ] {
            assert!(validate_credential_env(&[name.into()]).is_err());
        }
    }

    fn fixture() -> (
        tempfile::TempDir,
        crate::runtime::RuntimePaths,
        InvocationArgs,
    ) {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        for args in [
            ["init", "-b", "main"].as_slice(),
            ["config", "user.name", "Fixture"].as_slice(),
            ["config", "user.email", "fixture@localhost"].as_slice(),
        ] {
            assert!(std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap()
                .status
                .success());
        }
        std::fs::write(repo.join("file.txt"), "fixture\n").unwrap();
        for args in [
            ["add", "."].as_slice(),
            ["commit", "-m", "fixture"].as_slice(),
        ] {
            assert!(std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap()
                .status
                .success());
        }
        let request = ca_automation_protocol::MechanisticRequestV1 {
            schema: ca_automation_protocol::MECHANISTIC_REQUEST_SCHEMA.into(),
            expected_revision: AutomationEngine::repository_revision(&repo).unwrap(),
            delegation_policy: ca_automation_protocol::DelegationPolicy::Require,
            purpose: ca_automation_protocol::MechanisticRequestPurposeV1::Ordinary,
            allow_unisolated_evidence: false,
            actions: vec![ca_automation_protocol::MechanisticActionV1::GitHistory {
                max_entries: 3,
            }],
        };
        let input = root.join("operation.json");
        std::fs::write(
            &input,
            serde_json::to_vec(&EngineOperation::Mechanistic {
                repository: repo,
                request,
            })
            .unwrap(),
        )
        .unwrap();
        (
            temporary,
            crate::runtime::RuntimePaths::from_home(root.join("home")),
            InvocationArgs {
                input,
                run_id: "00000000-0000-4000-8000-000000000071".into(),
                runtime_seconds: 30,
                output_bytes: 4096,
                allow_network: false,
                sandbox: crate::sandbox::SandboxMode::Required,
                credential_env: Vec::new(),
            },
        )
    }

    #[test]
    fn preview_is_read_only_and_pins_occurrence_resources_and_policy() {
        let (_temporary, paths, mut args) = fixture();
        let first =
            load_invocation(&args, &paths, &crate::runtime::ActiveProfile::Default).unwrap();
        let second =
            load_invocation(&args, &paths, &crate::runtime::ActiveProfile::Default).unwrap();
        assert_eq!(first.confirmation_digest, second.confirmation_digest);
        assert!(
            !paths.home.exists(),
            "Preview must not create profile state or configuration"
        );
        args.allow_network = true;
        let changed =
            load_invocation(&args, &paths, &crate::runtime::ActiveProfile::Default).unwrap();
        assert_ne!(first.context.policy_digest, changed.context.policy_digest);
        assert_ne!(first.confirmation_digest, changed.confirmation_digest);
        args.run_id = "00000000-0000-4000-8000-000000000072".into();
        let occurrence =
            load_invocation(&args, &paths, &crate::runtime::ActiveProfile::Default).unwrap();
        assert_ne!(changed.confirmation_digest, occurrence.confirmation_digest);
        assert_eq!(
            changed.context.definition_digest,
            occurrence.context.definition_digest
        );
    }

    #[test]
    fn facade_cannot_select_foreign_profile_state_or_loader_environment() {
        let (_temporary, paths, mut args) = fixture();
        let invocation =
            load_invocation(&args, &paths, &crate::runtime::ActiveProfile::Default).unwrap();
        let mut context = invocation.context.clone();
        context.state_root = paths.home.join("foreign");
        assert!(
            require_profile(&context, &paths, &crate::runtime::ActiveProfile::Default).is_err()
        );
        args.credential_env = vec!["DYLD_INSERT_LIBRARIES".into()];
        assert!(load_invocation(&args, &paths, &crate::runtime::ActiveProfile::Default).is_err());
    }

    #[test]
    fn worker_binding_digest_survives_storage_creation_and_envelope_round_trip() {
        let (_temporary, paths, args) = fixture();
        let profile = crate::runtime::ActiveProfile::Default;
        let admitted = load_invocation(&args, &paths, &profile).unwrap();
        assert!(!paths.home.exists());
        let store = RunStore::open(&admitted.context).unwrap();
        let _resources = store.acquire_resources().unwrap();
        let worker: Invocation =
            serde_json::from_slice(&serde_json::to_vec(&admitted).unwrap()).unwrap();
        let rebound = make_invocation(worker.operation.clone(), &args, &paths, &profile).unwrap();
        assert_eq!(worker.context, rebound.context);
        assert_eq!(worker.confirmation_digest, rebound.confirmation_digest);
        assert!(changed_binding_fields(&worker, &rebound).is_empty());
        assert_eq!(
            serde_json::to_vec(&worker.context).unwrap(),
            serde_json::to_vec(&rebound.context).unwrap(),
            "serialized storage spelling must remain stable after admission"
        );
        let mut foreign = rebound;
        foreign.context.state_root = PathBuf::from("/private/provider-secret-state");
        assert_eq!(
            changed_binding_fields(&worker, &foreign),
            vec!["state_root"]
        );
    }

    #[test]
    #[cfg(unix)]
    fn worker_exit_and_exact_durable_receipt_are_required_for_acceptance() {
        use std::os::unix::process::ExitStatusExt;
        let (_temporary, paths, args) = fixture();
        let invocation =
            load_invocation(&args, &paths, &crate::runtime::ActiveProfile::Default).unwrap();
        let store = RunStore::open(&invocation.context).unwrap();
        store.start().unwrap();
        let persisted = store
            .finish(
                ca_automation::runtime::ExecutionState::Succeeded,
                Some(serde_json::json!({"fixture":"confirmed"})),
                None,
            )
            .unwrap();
        let output = serde_json::to_vec(&persisted).unwrap();
        let success = std::process::ExitStatus::from_raw(0);
        let failure = std::process::ExitStatus::from_raw(2 << 8);
        assert_eq!(
            validate_worker_receipt(&success, &output, &persisted).unwrap(),
            persisted
        );
        assert!(validate_worker_receipt(&failure, &output, &persisted)
            .unwrap_err()
            .contains("worker failed"));
        let mut foreign = persisted.clone();
        foreign.policy_digest = "a".repeat(64);
        assert!(validate_worker_receipt(
            &success,
            &serde_json::to_vec(&foreign).unwrap(),
            &persisted
        )
        .unwrap_err()
        .contains("differs from its durable receipt"));
        let diagnostic =
            validate_worker_receipt(&success, b"private-provider-payload", &persisted).unwrap_err();
        assert!(diagnostic.contains("line 1, column"));
        assert!(!diagnostic.contains("private-provider-payload"));
    }

    #[test]
    fn worker_failure_diagnostics_are_bounded_and_redact_credentials_and_controls() {
        let secret = "fixture-explicit-private-value";
        let diagnostics = format!(
            "\u{1b}[31mBinding failed: {secret} sk-fixture-provider-secret https://user:password@host.invalid/path\nAuthorization: Bearer confidential\n{}",
            "tail".repeat(4096)
        );
        let diagnostic = worker_failure("Worker failed", &diagnostics, &[secret.into()]);
        for omitted in [secret, "sk-fixture", "password", "confidential", "\u{1b}"] {
            assert!(!diagnostic.contains(omitted), "unsafe worker diagnostic");
        }
        assert!(diagnostic.contains("Binding failed"));
        assert!(diagnostic.chars().count() < 1200);
    }
}
