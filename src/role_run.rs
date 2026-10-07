use clap::{CommandFactory, Parser};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

#[cfg(test)]
const ROLE_RUN_SCHEMA_V1: &str = "ca.role_run.v1";
#[cfg(test)]
const ROLE_RUN_PREFLIGHT_SCHEMA_V1: &str = "ca.role_run_preflight.v1";
pub const MAX_ROLE_RUN_INPUT_BYTES: usize = 1024 * 1024;
pub const MAX_REQUEST_ID_BYTES: usize = 128;
pub const MAX_PROMPT_BYTES: usize = 256 * 1024;
pub const MAX_TOOLS: usize = 32;
pub const MAX_GRANTS: usize = 64;
pub const MAX_PREFLIGHT_OUTPUT_BYTES: usize = 1024 * 1024;

const MAX_PROVIDER_BYTES: usize = 128;
const MAX_MODEL_BYTES: usize = 256;
const MAX_GRANT_PATTERN_BYTES: usize = 1024;
const MAX_PREFLIGHT_ROUTES: usize = 64;
const MAX_PREFLIGHT_MODELS_PER_ROUTE: usize = 256;
const MAX_PREFLIGHT_MODELS_TOTAL: usize = 2_048;
const MAX_GRANT_SCAN_ENTRIES: usize = 100_000;
const MAX_ROLE_RUN_TOOL_PATH_BYTES: usize = 4 * 1024;
const MAX_ROLE_RUN_PATTERN_BYTES: usize = 4 * 1024;
const MAX_ROLE_RUN_GLOB_BYTES: usize = 1024;
const MAX_ROLE_RUN_TREE_ENTRIES: usize = 20_000;
const MAX_ROLE_RUN_LIST_ENTRIES: usize = 4_096;
const MAX_ROLE_RUN_DIRECTORY_NAMES_BYTES: usize = 4 * 1024 * 1024;
const MAX_ROLE_RUN_TREE_DEPTH: usize = 64;
const MAX_ROLE_RUN_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ROLE_RUN_TREE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ROLE_RUN_READ_RESULT_BYTES: usize = 64 * 1024;
const MAX_ROLE_RUN_SMALL_RESULT_BYTES: usize = 4 * 1024;
const MAX_ROLE_RUN_BLAME_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_ROLE_RUN_BLAME_ERROR_BYTES: usize = 16 * 1024;
const MAX_ROLE_RUN_PATCH_HUNKS: usize = 2_048;
const MAX_ROLE_RUN_PATCH_LINES: usize = 65_536;
const MAX_ROLE_RUN_SOURCE_LINES: usize = 262_144;
const ROLE_RUN_BLAME_CANCEL_GRACE: Duration = Duration::from_secs(2);
const READ_ONLY_TOOLS: [&str; 6] = [
    "read_file",
    "list_dir",
    "search_files",
    "glob",
    "read_todos",
    "blame",
];
const WRITE_TOOLS: [&str; 2] = ["apply_patch", "write_file"];
const ROLE_RUN_MAX_TURNS: usize = 64;
const ROLE_RUN_TOOL_TIMEOUT_SECONDS: u64 = 120;

#[derive(Debug, Parser)]
#[command(
    name = "ca role-run",
    about = "Run one restricted, process-scoped orchestrator role"
)]
struct RawRoleRunArgs {
    /// Select the required nonreserved named profile
    #[arg(long, value_name = "NAME")]
    profile: String,

    /// Validate the profile offline and emit one readiness object
    #[arg(long)]
    preflight: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRunCli {
    profile: String,
    preflight: bool,
}

pub fn parse_args<I, T>(args: I) -> Result<RoleRunCli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let raw = RawRoleRunArgs::try_parse_from(args)?;
    validate_named_profile(&raw.profile).map_err(|error| {
        RawRoleRunArgs::command().error(clap::error::ErrorKind::InvalidValue, error.to_string())
    })?;
    Ok(RoleRunCli {
        profile: raw.profile,
        preflight: raw.preflight,
    })
}

pub async fn run_cli(cli: RoleRunCli) -> i32 {
    if cli.preflight {
        run_preflight(&cli.profile)
    } else {
        run_role(cli).await
    }
}

fn run_preflight(profile_name: &str) -> i32 {
    let mut issues = Vec::new();
    let mut routes = Vec::new();
    let mut sandbox_required = false;

    let profile = validate_named_profile(profile_name).ok();
    let config_path = profile.as_ref().and_then(|profile| {
        crate::runtime::RuntimePaths::discover()
            .ok()
            .map(|paths| paths.for_profile(profile).config_file)
    });
    match config_path {
        None => issues.push(RoleRunPreflightIssue::ProfileUnavailable),
        Some(path) => match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                issues.push(RoleRunPreflightIssue::ProfileUnavailable);
            }
            Err(_) => issues.push(RoleRunPreflightIssue::InvalidConfiguration),
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                issues.push(RoleRunPreflightIssue::InvalidConfiguration);
            }
            Ok(_) => {
                match crate::config::ConfigStore::load_role_run_profile_shape(&path, profile_name) {
                    Ok(shape) => {
                        sandbox_required =
                            shape.sandbox_mode == crate::sandbox::SandboxMode::Required;
                        routes = shape
                            .providers
                            .into_iter()
                            .filter_map(|provider| {
                                let models = provider
                                    .models
                                    .into_iter()
                                    .filter(|model| model.profile.supports_tools != Some(false))
                                    .map(|model| model.id)
                                    .collect::<Vec<_>>();
                                (!models.is_empty()).then_some(RoleRunPreflightRoute {
                                    provider: provider.id,
                                    models,
                                })
                            })
                            .collect();
                        if !sandbox_required {
                            issues.push(RoleRunPreflightIssue::SandboxNotRequired);
                        }
                        if routes.is_empty() {
                            issues.push(RoleRunPreflightIssue::NoConfiguredRoutes);
                        }
                    }
                    Err(_) => issues.push(RoleRunPreflightIssue::InvalidConfiguration),
                }
            }
        },
    }

    let receipt = RoleRunPreflight::new(profile_name, sandbox_required, routes, issues)
        .unwrap_or_else(|_| {
            RoleRunPreflight::new(
                profile_name,
                false,
                Vec::new(),
                vec![RoleRunPreflightIssue::InvalidConfiguration],
            )
            .expect("static role-run preflight fallback is valid")
        });
    let mut stdout = std::io::stdout().lock();
    let exit = write_preflight_receipt(&receipt, &mut stdout);
    if exit != 0 {
        eprintln!("ca role-run: could not write the preflight receipt");
    }
    exit
}

fn write_preflight_receipt(receipt: &RoleRunPreflight, writer: &mut dyn Write) -> i32 {
    let result = serde_json::to_vec(receipt)
        .map_err(std::io::Error::other)
        .and_then(|mut bytes| {
            bytes.push(b'\n');
            writer.write_all(&bytes)
        })
        .and_then(|()| writer.flush());
    match result {
        Ok(()) => 0,
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => 0,
        Err(_) => 1,
    }
}

async fn run_role(cli: RoleRunCli) -> i32 {
    let run_id = uuid::Uuid::new_v4();
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();

    let current_dir = match std::env::current_dir() {
        Ok(path) => path,
        Err(_) => {
            return emit_role_failure(
                run_id,
                crate::headless::HeadlessFailureKind::Input,
                "role-run current directory is unavailable",
                2,
                &mut stdout,
                &mut stderr,
            )
        }
    };
    let request = match read_and_validate_request(std::io::stdin().lock(), &current_dir) {
        Ok(request) => request,
        Err(error) => {
            return emit_role_failure(
                run_id,
                crate::headless::HeadlessFailureKind::Input,
                error.issue().message,
                2,
                &mut stdout,
                &mut stderr,
            )
        }
    };

    let profile = match validate_named_profile(&cli.profile) {
        Ok(profile) => profile,
        Err(error) => {
            return emit_role_failure(
                run_id,
                crate::headless::HeadlessFailureKind::Input,
                error.issue().message,
                2,
                &mut stdout,
                &mut stderr,
            )
        }
    };
    let runtime_paths = match crate::runtime::RuntimePaths::discover() {
        Ok(paths) => paths.for_profile(&profile),
        Err(_) => {
            return emit_role_failure(
                run_id,
                crate::headless::HeadlessFailureKind::Setup,
                "role-run profile is unavailable",
                3,
                &mut stdout,
                &mut stderr,
            )
        }
    };
    let store = match crate::config::ConfigStore::for_role_run_execution(
        runtime_paths.config_file,
        &cli.profile,
    ) {
        Ok(store) => store,
        Err(_) => {
            return emit_role_failure(
                run_id,
                crate::headless::HeadlessFailureKind::Setup,
                "role-run profile is unavailable",
                3,
                &mut stdout,
                &mut stderr,
            )
        }
    };
    let route = match store.load_role_run_selected_route(
        request.provider(),
        request.model(),
        Some(request.reasoning_effort()),
    ) {
        Ok(route) => route,
        Err(_) => {
            return emit_role_failure(
                run_id,
                crate::headless::HeadlessFailureKind::Setup,
                "role-run selected route is unavailable",
                3,
                &mut stdout,
                &mut stderr,
            )
        }
    };
    let policy = crate::permission::role_run_permission_policy(
        &route.legacy_approval_rules,
        &route.permission_rules,
    );
    let capabilities = crate::runtime::RunCapabilities::role_run();
    let tools_supported = route.model_profile.supports_tools != Some(false);
    let tool_policy = match crate::tool_selection::ToolSelectionPolicy::from_cli(
        Some(request.tools().to_vec()),
        Vec::new(),
        Vec::new(),
    ) {
        Ok(policy) => policy,
        Err(_) => {
            return emit_role_failure(
                run_id,
                crate::headless::HeadlessFailureKind::Input,
                "role-run tools are invalid",
                2,
                &mut stdout,
                &mut stderr,
            )
        }
    };
    let chat_options = crate::agent::client::ChatRequestOptions {
        reasoning_effort: route.reasoning_effort,
        reasoning_request_format: route
            .reasoning_effort
            .and(route.model_profile.reasoning_request_format),
        reasoning_stream_field: route.model_profile.reasoning_stream_field.clone(),
        tool_call_format: route.model_profile.tool_call_format,
    };
    let output_redactions =
        crate::headless::HeadlessOutputRedactions::exact([route.provider.api_key.clone()]);
    let client = crate::agent::AgentClient::with_request_store(
        route.provider_id.clone(),
        &route.provider,
        &route.model_id,
        crate::request::RequestStore::default(),
    );
    crate::scratch::set_active(crate::scratch::ScratchStore::unavailable(
        request.workspace(),
    ));
    let skills = std::sync::Arc::new(std::sync::Mutex::new(crate::skills::SkillRegistry::empty()));
    let system = format!(
        "You are executing supervised orchestrator role {} in {}. Use only the advertised tools and exact process grants. Do not attempt unavailable capabilities, external communication, or network tools. Complete the supplied role assignment, then call report_structured_output exactly once with the final object.",
        request.request_id(),
        request.workspace().display()
    );
    let messages = vec![
        crate::agent::client::ChatMessage {
            role: "system".into(),
            content: crate::image_content::ModelContent::text(system.clone()),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
        crate::agent::client::ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text(request.prompt().to_string()),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
    ];
    let allowed_names = request.tools().iter().cloned().collect::<HashSet<_>>();
    let exit = crate::headless::run_with_backend(
        &client,
        messages,
        crate::headless::HeadlessRunConfig {
            workspace_activity_scope: Some(runtime_paths.data_dir.clone()),
            automation_policy: None,
            outcome_sink: None,
            provider: route.provider_id,
            model: route.model_id,
            cwd: request.workspace().to_path_buf(),
            workspace: crate::workspace::WorkspaceContext::from_primary(
                request.workspace().to_path_buf(),
                request.workspace().to_path_buf(),
            ),
            max_turns: ROLE_RUN_MAX_TURNS,
            single_turn: false,
            command_timeout_secs: ROLE_RUN_TOOL_TIMEOUT_SECONDS,
            command_output_policy: crate::config::CommandOutputPolicy::Bounded,
            output_format: crate::headless::OutputFormat::Jsonl,
            show_thinking: false,
            printenv: false,
            approval_mode: crate::headless::HeadlessApprovalMode::RequireApproval,
            approval_rules: policy.legacy_rules,
            permission_rules: policy.rules,
            session_grants: request.permission_grants().to_vec(),
            allow_network: capabilities.network,
            output_last_message: None,
            check_mode: crate::headless_contract::CheckMode::Disabled,
            convergence: None,
            structured_output: Some(request.structured_output_contract().clone()),
            original_request: request.prompt().to_string(),
            verification_instructions: system,
            chat_options,
            sandbox_mode: route.sandbox_mode,
            run_id,
            job_owner: crate::jobs::JobOwner::new(None),
            skills,
            mcp: None,
            mechanistic: crate::config::MechanisticMcpConfig::default(),
            lsp: None,
            email: None,
            tool_policy,
            tool_constraints: crate::tool_selection::ToolSelectionConstraints {
                tools_supported,
                images_supported: false,
                skills_enabled: capabilities.skills,
                skill_activation_ids: None,
                skill_view_ids: None,
                mcp_enabled: capabilities.mcp,
                mechanistic_delegation_enabled: false,
                transparent_mechanistic_enabled: false,
                remote_mechanistic_enabled: false,
                automation_setup_enabled: false,
                lsp_enabled: capabilities.lsp,
                memory_generation_enabled: capabilities.memory,
                automation_proposals_enabled: false,
                email_enabled: capabilities.email,
                allowed_names: Some(allowed_names),
            },
            tool_profile: crate::agent::registry::ToolProfile::Main,
            startup_warnings: Vec::new(),
            request_kind: crate::request::RequestKind::RoleRun,
            agents: None,
            work_state: std::sync::Arc::new(std::sync::Mutex::new(
                crate::work::WorkState::default(),
            )),
            role_run_workspace_identity: Some(request.workspace_identity()),
            output_redactions,
        },
        &mut stdout,
        &mut stderr,
    )
    .await;
    exit.code
}

fn emit_role_failure(
    run_id: uuid::Uuid,
    kind: crate::headless::HeadlessFailureKind,
    message: &str,
    code: i32,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    crate::headless::emit_standalone_failure_for_contract(
        crate::headless::OutputFormat::Jsonl,
        run_id,
        true,
        kind,
        message.to_string(),
        code,
        (stdout, stderr),
    )
    .code
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum RoleRunSchema {
    #[serde(rename = "ca.role_run.v1")]
    V1,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RoleRunRequest {
    schema: RoleRunSchema,
    request_id: String,
    provider: String,
    model: String,
    reasoning_effort: crate::config::ReasoningEffort,
    workspace: PathBuf,
    prompt: String,
    output_schema: PathBuf,
    tools: Vec<String>,
    grants: Vec<RoleRunGrant>,
}

impl std::fmt::Debug for RoleRunRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RoleRunRequest")
            .field("schema", &self.schema)
            .field("request_id", &self.request_id)
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("workspace", &self.workspace)
            .field("prompt", &"[ROLE-RUN PROMPT REDACTED]")
            .field("output_schema", &self.output_schema)
            .field("tool_count", &self.tools.len())
            .field("grant_count", &self.grants.len())
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoleRunGrant {
    Tool { tool: String },
    Path { tool: String, path_pattern: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum RoleRunPreflightSchema {
    #[serde(rename = "ca.role_run_preflight.v1")]
    V1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RoleRunPreflightRoute {
    pub provider: String,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RoleRunPreflightIssue {
    ProfileUnavailable,
    InvalidConfiguration,
    SandboxNotRequired,
    NoConfiguredRoutes,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RoleRunPreflight {
    schema: RoleRunPreflightSchema,
    pub ready: bool,
    pub profile: String,
    pub sandbox_required: bool,
    pub routes: Vec<RoleRunPreflightRoute>,
    pub issues: Vec<RoleRunPreflightIssue>,
}

impl RoleRunPreflight {
    pub fn new(
        profile: &str,
        sandbox_required: bool,
        routes: Vec<RoleRunPreflightRoute>,
        issues: Vec<RoleRunPreflightIssue>,
    ) -> Result<Self, RoleRunError> {
        validate_named_profile(profile)?;
        validate_preflight_routes(&routes)?;
        if issues.len() > 32 || issues.iter().copied().collect::<HashSet<_>>().len() != issues.len()
        {
            return Err(RoleRunError::InvalidPreflight);
        }
        let ready = sandbox_required && !routes.is_empty() && issues.is_empty();
        let receipt = Self {
            schema: RoleRunPreflightSchema::V1,
            ready,
            profile: profile.to_string(),
            sandbox_required,
            routes,
            issues,
        };
        if serde_json::to_vec(&receipt)
            .map_err(|_| RoleRunError::InvalidPreflight)?
            .len()
            > MAX_PREFLIGHT_OUTPUT_BYTES
        {
            return Err(RoleRunError::InvalidPreflight);
        }
        Ok(receipt)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RoleRunError {
    #[error("role-run input could not be read")]
    InputRead,
    #[error("role-run input exceeds the 1 MiB limit")]
    InputTooLarge,
    #[error("role-run input is not an exact ca.role_run.v1 object")]
    InvalidRequestEnvelope,
    #[error("role-run requires a nonreserved named profile")]
    InvalidProfile,
    #[error("role-run request_id is not bounded safe ASCII")]
    InvalidRequestId,
    #[error("role-run provider or model identifier is invalid")]
    InvalidRoute,
    #[error("role-run prompt is empty or exceeds the 256 KiB limit")]
    InvalidPrompt,
    #[error("role-run tools are outside the fixed v1 allowlist or malformed")]
    InvalidTools,
    #[error("role-run grants are incomplete, malformed, or outside the workspace")]
    InvalidGrants,
    #[error("role-run workspace must be the canonical current directory")]
    InvalidWorkspace,
    #[error("role-run output schema must be a contained regular non-symlink schema")]
    InvalidOutputSchema,
    #[error("role-run preflight receipt is invalid")]
    InvalidPreflight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RoleRunInvocationError {
    #[error("role-run tool invocation is unavailable")]
    UnsupportedTool,
    #[error("role-run tool arguments are invalid")]
    InvalidArguments,
    #[error("role-run tool path is invalid or outside the workspace")]
    InvalidPath,
}

/// Recheck process-scoped role grants with path semantics that do not let a
/// single `*` cross a directory separator or match a hidden path component.
/// The ordinary permission broker still applies persistent Deny ceilings;
/// this is an additional least-authority gate for role-run positive grants.
pub fn role_run_process_grants_authorize(
    workspace: &Path,
    grants: &[crate::permission::PermissionMatcher],
    tool: &str,
    arguments: &serde_json::Value,
) -> bool {
    if READ_ONLY_TOOLS.contains(&tool) {
        return grants.iter().any(|grant| {
            matches!(
                grant,
                crate::permission::PermissionMatcher::Tool { tool: granted }
                    if granted == tool
            )
        });
    }
    if !WRITE_TOOLS.contains(&tool) {
        return false;
    }

    let Some(path) = arguments
        .get("path")
        .and_then(serde_json::Value::as_str)
        .map(Path::new)
        .filter(|path| path.is_absolute() && path.starts_with(workspace))
    else {
        return false;
    };
    let Ok(relative) = path.strip_prefix(workspace) else {
        return false;
    };
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty()
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
        || components.iter().any(|component| {
            let Component::Normal(component) = component else {
                return true;
            };
            component.to_str().is_some_and(|component| {
                component.eq_ignore_ascii_case(".git")
                    || component.eq_ignore_ascii_case(".orchestrator")
            })
        })
        || components.first().is_some_and(|component| {
            let Component::Normal(component) = component else {
                return true;
            };
            component.to_str().is_some_and(|component| {
                component.eq_ignore_ascii_case("oracles")
                    || component.eq_ignore_ascii_case("reference")
            })
        })
    {
        return false;
    }

    let options = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: true,
    };
    grants.iter().any(|grant| {
        let crate::permission::PermissionMatcher::Path {
            tool: granted,
            path_pattern,
        } = grant
        else {
            return false;
        };
        granted == tool
            && glob::Pattern::new(path_pattern)
                .is_ok_and(|pattern| pattern.matches_path_with(path, options))
    })
}

impl RoleRunError {
    pub fn issue(self) -> RoleRunValidationIssue {
        let code = match self {
            Self::InputRead => "input_read_failed",
            Self::InputTooLarge => "input_too_large",
            Self::InvalidRequestEnvelope => "invalid_request_envelope",
            Self::InvalidProfile => "invalid_profile",
            Self::InvalidRequestId => "invalid_request_id",
            Self::InvalidRoute => "invalid_route",
            Self::InvalidPrompt => "invalid_prompt",
            Self::InvalidTools => "invalid_tools",
            Self::InvalidGrants => "invalid_grants",
            Self::InvalidWorkspace => "invalid_workspace",
            Self::InvalidOutputSchema => "invalid_output_schema",
            Self::InvalidPreflight => "invalid_preflight",
        };
        RoleRunValidationIssue {
            code,
            message: match self {
                Self::InputRead => "role-run input could not be read",
                Self::InputTooLarge => "role-run input exceeds the 1 MiB limit",
                Self::InvalidRequestEnvelope => {
                    "role-run input is not an exact ca.role_run.v1 object"
                }
                Self::InvalidProfile => "role-run requires a nonreserved named profile",
                Self::InvalidRequestId => "role-run request_id is not bounded safe ASCII",
                Self::InvalidRoute => "role-run provider or model identifier is invalid",
                Self::InvalidPrompt => "role-run prompt is empty or exceeds the 256 KiB limit",
                Self::InvalidTools => {
                    "role-run tools are outside the fixed v1 allowlist or malformed"
                }
                Self::InvalidGrants => {
                    "role-run grants are incomplete, malformed, or outside the workspace"
                }
                Self::InvalidWorkspace => {
                    "role-run workspace must be the canonical current directory"
                }
                Self::InvalidOutputSchema => {
                    "role-run output schema must be a contained regular non-symlink schema"
                }
                Self::InvalidPreflight => "role-run preflight receipt is invalid",
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct RoleRunValidationIssue {
    pub code: &'static str,
    pub message: &'static str,
}

pub struct ValidatedRoleRunRequest {
    request: RoleRunRequest,
    permission_grants: Vec<crate::permission::PermissionMatcher>,
    structured_output: crate::headless_contract::StructuredOutputContract,
    workspace_identity: RoleRunWorkspaceIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleRunWorkspaceIdentity {
    device: u64,
    inode: u64,
}

impl std::fmt::Debug for ValidatedRoleRunRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ValidatedRoleRunRequest")
            .field("request", &self.request)
            .field("permission_grant_count", &self.permission_grants.len())
            .field("structured_output", &"[VALIDATED SCHEMA]")
            .finish()
    }
}

impl ValidatedRoleRunRequest {
    pub fn request_id(&self) -> &str {
        &self.request.request_id
    }

    pub fn provider(&self) -> &str {
        &self.request.provider
    }

    pub fn model(&self) -> &str {
        &self.request.model
    }

    pub fn reasoning_effort(&self) -> crate::config::ReasoningEffort {
        self.request.reasoning_effort
    }

    pub fn workspace(&self) -> &Path {
        &self.request.workspace
    }

    pub fn workspace_identity(&self) -> RoleRunWorkspaceIdentity {
        self.workspace_identity
    }

    pub fn prompt(&self) -> &str {
        &self.request.prompt
    }

    pub fn tools(&self) -> &[String] {
        &self.request.tools
    }

    pub fn permission_grants(&self) -> &[crate::permission::PermissionMatcher] {
        &self.permission_grants
    }

    pub fn structured_output_contract(
        &self,
    ) -> &crate::headless_contract::StructuredOutputContract {
        &self.structured_output
    }
}

pub fn validate_named_profile(
    profile: &str,
) -> Result<crate::runtime::ActiveProfile, RoleRunError> {
    match crate::runtime::ActiveProfile::parse(Some(profile)) {
        Ok(profile @ crate::runtime::ActiveProfile::Named(_)) => Ok(profile),
        Ok(crate::runtime::ActiveProfile::Default | crate::runtime::ActiveProfile::Bare)
        | Err(_) => Err(RoleRunError::InvalidProfile),
    }
}

pub fn parse_request(mut reader: impl Read) -> Result<RoleRunRequest, RoleRunError> {
    let mut bytes = Vec::with_capacity(64 * 1024);
    reader
        .by_ref()
        .take((MAX_ROLE_RUN_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| RoleRunError::InputRead)?;
    if bytes.len() > MAX_ROLE_RUN_INPUT_BYTES {
        return Err(RoleRunError::InputTooLarge);
    }
    serde_json::from_slice(&bytes).map_err(|_| RoleRunError::InvalidRequestEnvelope)
}

pub fn read_and_validate_request(
    reader: impl Read,
    current_dir: &Path,
) -> Result<ValidatedRoleRunRequest, RoleRunError> {
    validate_request(parse_request(reader)?, current_dir)
}

pub fn validate_request(
    request: RoleRunRequest,
    current_dir: &Path,
) -> Result<ValidatedRoleRunRequest, RoleRunError> {
    validate_request_text(&request)?;
    let workspace = validate_workspace(&request.workspace, current_dir)?;
    let workspace_identity = capture_role_run_workspace_identity(&workspace)?;
    let permission_grants = validate_authority(&workspace, &request.tools, &request.grants)?;
    let structured_output =
        validate_output_schema(&workspace, &request.output_schema, workspace_identity)?;
    Ok(ValidatedRoleRunRequest {
        request,
        permission_grants,
        structured_output,
        workspace_identity,
    })
}

#[cfg(unix)]
fn capture_role_run_workspace_identity(
    workspace: &Path,
) -> Result<RoleRunWorkspaceIdentity, RoleRunError> {
    use std::os::unix::fs::MetadataExt;

    let metadata =
        std::fs::symlink_metadata(workspace).map_err(|_| RoleRunError::InvalidWorkspace)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RoleRunError::InvalidWorkspace);
    }
    Ok(RoleRunWorkspaceIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(not(unix))]
fn capture_role_run_workspace_identity(
    _workspace: &Path,
) -> Result<RoleRunWorkspaceIdentity, RoleRunError> {
    Err(RoleRunError::InvalidWorkspace)
}

fn validate_request_text(request: &RoleRunRequest) -> Result<(), RoleRunError> {
    let valid_id = !request.request_id.is_empty()
        && request.request_id.len() <= MAX_REQUEST_ID_BYTES
        && request
            .request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if !valid_id {
        return Err(RoleRunError::InvalidRequestId);
    }
    if !valid_provider(&request.provider) || !valid_model(&request.model) {
        return Err(RoleRunError::InvalidRoute);
    }
    if request.prompt.trim().is_empty() || request.prompt.len() > MAX_PROMPT_BYTES {
        return Err(RoleRunError::InvalidPrompt);
    }
    validate_tools(&request.tools)
}

fn valid_provider(provider: &str) -> bool {
    !provider.is_empty()
        && provider.len() <= MAX_PROVIDER_BYTES
        && provider
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_model(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= MAX_MODEL_BYTES
        && model.trim() == model
        && !model.chars().any(char::is_control)
}

fn validate_tools(tools: &[String]) -> Result<(), RoleRunError> {
    if tools.is_empty() || tools.len() > MAX_TOOLS {
        return Err(RoleRunError::InvalidTools);
    }
    let mut seen = HashSet::with_capacity(tools.len());
    for tool in tools {
        if !seen.insert(tool.as_str())
            || (!READ_ONLY_TOOLS.contains(&tool.as_str()) && !WRITE_TOOLS.contains(&tool.as_str()))
        {
            return Err(RoleRunError::InvalidTools);
        }
    }
    let apply_patch = seen.contains("apply_patch");
    let write_file = seen.contains("write_file");
    if apply_patch != write_file {
        return Err(RoleRunError::InvalidTools);
    }
    Ok(())
}

fn validate_workspace(workspace: &Path, current_dir: &Path) -> Result<PathBuf, RoleRunError> {
    if !workspace.is_absolute() || !current_dir.is_absolute() {
        return Err(RoleRunError::InvalidWorkspace);
    }
    for path in [workspace, current_dir] {
        let normalized = path.components().collect::<PathBuf>();
        if normalized.as_os_str().as_encoded_bytes() != path.as_os_str().as_encoded_bytes()
            || path.components().any(|component| {
                !matches!(
                    component,
                    Component::Prefix(_) | Component::RootDir | Component::Normal(_)
                )
            })
        {
            return Err(RoleRunError::InvalidWorkspace);
        }
    }
    let workspace_canonical = workspace
        .canonicalize()
        .map_err(|_| RoleRunError::InvalidWorkspace)?;
    let current_canonical = current_dir
        .canonicalize()
        .map_err(|_| RoleRunError::InvalidWorkspace)?;
    if workspace_canonical != workspace
        || current_canonical != current_dir
        || workspace_canonical != current_canonical
        || !workspace_canonical.is_dir()
        || workspace_canonical
            .to_str()
            .is_none_or(|path| path.chars().any(is_glob_meta))
    {
        return Err(RoleRunError::InvalidWorkspace);
    }
    Ok(workspace_canonical)
}

fn validate_authority(
    workspace: &Path,
    tools: &[String],
    grants: &[RoleRunGrant],
) -> Result<Vec<crate::permission::PermissionMatcher>, RoleRunError> {
    if grants.is_empty() || grants.len() > MAX_GRANTS {
        return Err(RoleRunError::InvalidGrants);
    }
    let tool_set = tools.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut seen = HashSet::with_capacity(grants.len());
    let mut granted_tools = HashSet::with_capacity(tool_set.len());
    let mut permission_grants = Vec::with_capacity(grants.len());
    for grant in grants {
        if !seen.insert(grant) {
            return Err(RoleRunError::InvalidGrants);
        }
        let matcher = match grant {
            RoleRunGrant::Tool { tool }
                if tool_set.contains(tool.as_str()) && READ_ONLY_TOOLS.contains(&tool.as_str()) =>
            {
                crate::permission::PermissionMatcher::Tool { tool: tool.clone() }
            }
            RoleRunGrant::Path { tool, path_pattern }
                if tool_set.contains(tool.as_str()) && WRITE_TOOLS.contains(&tool.as_str()) =>
            {
                validate_grant_path(workspace, path_pattern)?;
                crate::permission::PermissionMatcher::Path {
                    tool: tool.clone(),
                    path_pattern: path_pattern.clone(),
                }
            }
            _ => return Err(RoleRunError::InvalidGrants),
        };
        matcher
            .validate()
            .map_err(|_| RoleRunError::InvalidGrants)?;
        let granted_tool = match grant {
            RoleRunGrant::Tool { tool } | RoleRunGrant::Path { tool, .. } => tool.as_str(),
        };
        granted_tools.insert(granted_tool);
        permission_grants.push(matcher);
    }
    if granted_tools != tool_set {
        return Err(RoleRunError::InvalidGrants);
    }
    Ok(permission_grants)
}

fn validate_grant_path(workspace: &Path, pattern: &str) -> Result<(), RoleRunError> {
    if pattern.is_empty()
        || pattern.len() > MAX_GRANT_PATTERN_BYTES
        || pattern.contains('\\')
        || pattern.chars().any(char::is_control)
    {
        return Err(RoleRunError::InvalidGrants);
    }
    let workspace = workspace.to_str().ok_or(RoleRunError::InvalidWorkspace)?;
    let suffix = pattern
        .strip_prefix(workspace)
        .filter(|suffix| suffix.starts_with('/'))
        .and_then(|suffix| suffix.strip_prefix('/'))
        .filter(|suffix| !suffix.is_empty())
        .ok_or(RoleRunError::InvalidGrants)?;
    let components = suffix.split('/').collect::<Vec<_>>();
    if components
        .iter()
        .any(|component| component.is_empty() || matches!(*component, "." | ".."))
        || components
            .first()
            .is_none_or(|component| component.chars().any(is_glob_meta))
        || components.first().is_some_and(|component| {
            [".git", ".orchestrator", "oracles", "reference"]
                .iter()
                .any(|protected| component.eq_ignore_ascii_case(protected))
        })
    {
        return Err(RoleRunError::InvalidGrants);
    }
    reject_existing_symlink_scope(Path::new(workspace), &components)
}

fn reject_existing_symlink_scope(
    workspace: &Path,
    components: &[&str],
) -> Result<(), RoleRunError> {
    let mut path = workspace.to_path_buf();
    let mut glob_index = None;
    for (index, component) in components.iter().enumerate() {
        if component.chars().any(is_glob_meta) {
            glob_index = Some(index);
            break;
        }
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(RoleRunError::InvalidGrants)
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(RoleRunError::InvalidGrants),
        }
    }
    if glob_index.is_some() && path.exists() {
        reject_symlinks_below(&path)?;
    }
    Ok(())
}

fn reject_symlinks_below(root: &Path) -> Result<(), RoleRunError> {
    let metadata = std::fs::symlink_metadata(root).map_err(|_| RoleRunError::InvalidGrants)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RoleRunError::InvalidGrants);
    }
    let mut seen = 0usize;
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory).map_err(|_| RoleRunError::InvalidGrants)? {
            let entry = entry.map_err(|_| RoleRunError::InvalidGrants)?;
            seen = seen
                .checked_add(1)
                .filter(|seen| *seen <= MAX_GRANT_SCAN_ENTRIES)
                .ok_or(RoleRunError::InvalidGrants)?;
            let metadata =
                std::fs::symlink_metadata(entry.path()).map_err(|_| RoleRunError::InvalidGrants)?;
            if metadata.file_type().is_symlink() {
                return Err(RoleRunError::InvalidGrants);
            }
            if metadata.is_dir() {
                directories.push(entry.path());
            }
        }
    }
    Ok(())
}

/// Validate and normalize one model-supplied role-run tool invocation.
///
/// This boundary is deliberately stricter than the general interactive tool
/// preparation path. It uses the exact role workspace, rejects lexical
/// traversal and symlink components, rejects special files (including FIFOs),
/// and bounds every directory walk before the ordinary tool executor can see
/// the invocation. Returned path arguments are absolute and canonical in
/// spelling, but no symlink is followed to obtain them.
pub fn validate_role_run_tool_invocation(
    workspace: &Path,
    tool: &str,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, RoleRunInvocationError> {
    let workspace = validate_workspace(workspace, workspace)
        .map_err(|_| RoleRunInvocationError::InvalidPath)?;
    crate::agent::registry::ToolRegistry::builtins()
        .validate(tool, arguments)
        .map_err(|_| RoleRunInvocationError::InvalidArguments)?;
    let object = arguments
        .as_object()
        .ok_or(RoleRunInvocationError::InvalidArguments)?;

    match tool {
        "apply_patch" => {
            require_exact_argument_keys(object, &["path", "patch"])?;
        }
        "write_file" => {
            require_exact_argument_keys(object, &["path", "content"])?;
        }
        "read_file" => {
            require_exact_argument_keys(object, &["path", "start_line", "end_line"])?;
            validate_read_file_range(object)?;
        }
        "list_dir" => require_exact_argument_keys(object, &["path"])?,
        "search_files" => {
            require_exact_argument_keys(object, &["pattern", "path"])?;
            validate_search_pattern(object)?;
        }
        "glob" => {
            require_exact_argument_keys(object, &["pattern", "path"])?;
            validate_glob_pattern(object)?;
        }
        "read_todos" => require_exact_argument_keys(object, &["path"])?,
        "blame" => {
            require_exact_argument_keys(object, &["path", "start_line", "end_line"])?;
            validate_blame_range(object)?;
        }
        _ => return Err(RoleRunInvocationError::UnsupportedTool),
    }

    let path_required = match tool {
        "apply_patch" | "write_file" | "read_file" | "blame" | "list_dir" => true,
        "search_files" | "read_todos" | "glob" => false,
        _ => unreachable!("the role-run tool match above is exhaustive"),
    };
    let raw_path = object
        .get("path")
        .and_then(serde_json::Value::as_str)
        .filter(|path| !path.is_empty());
    if path_required && raw_path.is_none() {
        return Err(RoleRunInvocationError::InvalidArguments);
    }
    // The approval phase needs a normalized contained spelling. Existing
    // directory components are walked through no-follow descriptors so an
    // alternate-case alias cannot bypass a Deny ceiling. Final-node type,
    // link count, tree bounds, and glob traversal remain authoritative in the
    // descriptor-rooted execution boundary.
    let path = resolve_role_run_path(&workspace, raw_path.unwrap_or("."))?;

    let mut normalized = arguments.clone();
    normalized
        .as_object_mut()
        .expect("validated tool arguments are an object")
        .insert(
            "path".into(),
            serde_json::Value::String(path.to_string_lossy().into_owned()),
        );
    Ok(normalized)
}

fn require_exact_argument_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    allowed: &[&str],
) -> Result<(), RoleRunInvocationError> {
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(RoleRunInvocationError::InvalidArguments);
    }
    Ok(())
}

fn validate_read_file_range(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), RoleRunInvocationError> {
    let start = object.get("start_line").and_then(serde_json::Value::as_u64);
    let end = object.get("end_line").and_then(serde_json::Value::as_u64);
    let start = start.unwrap_or(1);
    let end = end.unwrap_or_else(|| start.saturating_add(199));
    if start == 0 || end < start || end.saturating_sub(start).saturating_add(1) > 500 {
        return Err(RoleRunInvocationError::InvalidArguments);
    }
    Ok(())
}

fn validate_blame_range(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), RoleRunInvocationError> {
    let start = object.get("start_line").and_then(serde_json::Value::as_u64);
    let end = object.get("end_line").and_then(serde_json::Value::as_u64);
    match (start, end) {
        (None, None) => Ok(()),
        (Some(start), Some(end))
            if start > 0 && end >= start && end.saturating_sub(start) < 500 =>
        {
            Ok(())
        }
        _ => Err(RoleRunInvocationError::InvalidArguments),
    }
}

fn validate_search_pattern(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), RoleRunInvocationError> {
    let pattern = object
        .get("pattern")
        .and_then(serde_json::Value::as_str)
        .filter(|pattern| {
            !pattern.is_empty()
                && pattern.len() <= MAX_ROLE_RUN_PATTERN_BYTES
                && !pattern.chars().any(char::is_control)
        })
        .ok_or(RoleRunInvocationError::InvalidArguments)?;
    regex::RegexBuilder::new(pattern)
        .size_limit(4 * 1024 * 1024)
        .dfa_size_limit(2 * 1024 * 1024)
        .build()
        .map_err(|_| RoleRunInvocationError::InvalidArguments)?;
    Ok(())
}

fn validate_glob_pattern(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), RoleRunInvocationError> {
    let pattern = object
        .get("pattern")
        .and_then(serde_json::Value::as_str)
        .filter(|pattern| {
            !pattern.is_empty()
                && pattern.len() <= MAX_ROLE_RUN_GLOB_BYTES
                && !pattern.starts_with('/')
                && !pattern.contains('\\')
                && !pattern.chars().any(char::is_control)
        })
        .ok_or(RoleRunInvocationError::InvalidArguments)?;
    let components = pattern.split('/').collect::<Vec<_>>();
    if components
        .iter()
        .any(|component| component.is_empty() || matches!(*component, "." | ".."))
        || components.iter().any(|component| {
            matches!(
                component.to_ascii_lowercase().as_str(),
                ".git" | ".orchestrator"
            )
        })
    {
        return Err(RoleRunInvocationError::InvalidArguments);
    }
    glob::Pattern::new(pattern).map_err(|_| RoleRunInvocationError::InvalidArguments)?;
    let private_match_options = glob::MatchOptions {
        case_sensitive: false,
        require_literal_leading_dot: true,
        ..glob::MatchOptions::new()
    };
    for component in components {
        let compiled =
            glob::Pattern::new(component).map_err(|_| RoleRunInvocationError::InvalidArguments)?;
        if (component.starts_with('.') && (compiled.matches(".") || compiled.matches("..")))
            || compiled.matches_with(".git", private_match_options)
            || compiled.matches_with(".orchestrator", private_match_options)
        {
            return Err(RoleRunInvocationError::InvalidArguments);
        }
    }
    Ok(())
}

fn resolve_role_run_path(workspace: &Path, raw: &str) -> Result<PathBuf, RoleRunInvocationError> {
    if raw.is_empty()
        || raw.len() > MAX_ROLE_RUN_TOOL_PATH_BYTES
        || raw.starts_with('~')
        || raw.contains('\\')
        || raw.chars().any(char::is_control)
        || raw.chars().any(is_glob_meta)
    {
        return Err(RoleRunInvocationError::InvalidPath);
    }
    let supplied = Path::new(raw);
    let relative = if supplied.is_absolute() {
        supplied
            .strip_prefix(workspace)
            .map_err(|_| RoleRunInvocationError::InvalidPath)?
    } else if raw == "." {
        Path::new("")
    } else {
        supplied
    };
    let components = relative.components().collect::<Vec<_>>();
    if components.len() > MAX_ROLE_RUN_TREE_DEPTH
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
        || components.iter().any(|component| {
            let Component::Normal(component) = component else {
                return false;
            };
            component
                .to_str()
                .is_some_and(is_private_role_run_path_name)
        })
    {
        return Err(RoleRunInvocationError::InvalidPath);
    }
    require_exact_existing_path_spelling(workspace, relative)?;

    Ok(if relative.as_os_str().is_empty() {
        workspace.to_path_buf()
    } else {
        workspace.join(relative)
    })
}

#[cfg(unix)]
fn require_exact_existing_path_spelling(
    workspace: &Path,
    relative: &Path,
) -> Result<(), RoleRunInvocationError> {
    let expected_workspace = capture_role_run_workspace_identity(workspace)
        .map_err(|_| RoleRunInvocationError::InvalidPath)?;
    let mut directory = open_role_run_workspace_root(workspace, expected_workspace)
        .map_err(|_| RoleRunInvocationError::InvalidPath)?;
    let components = relative.components().collect::<Vec<_>>();
    let cancellation = std::sync::atomic::AtomicBool::new(false);
    let mut total_seen = 0usize;

    for (index, component) in components.iter().enumerate() {
        let Component::Normal(expected) = component else {
            return Err(RoleRunInvocationError::InvalidPath);
        };
        let expected = expected
            .to_str()
            .ok_or(RoleRunInvocationError::InvalidPath)?;
        let names =
            read_role_run_directory_names(&directory, MAX_ROLE_RUN_LIST_ENTRIES, &cancellation)
                .map_err(|_| RoleRunInvocationError::InvalidPath)?;
        total_seen = total_seen
            .checked_add(names.len())
            .filter(|seen| *seen <= MAX_ROLE_RUN_TREE_ENTRIES)
            .ok_or(RoleRunInvocationError::InvalidPath)?;
        if !names.iter().any(|name| name == expected) {
            if role_run_entry_exists_nofollow(&directory, expected)? {
                return Err(RoleRunInvocationError::InvalidPath);
            }
            return if index + 1 == components.len() {
                Ok(())
            } else {
                Err(RoleRunInvocationError::InvalidPath)
            };
        }

        if index + 1 < components.len() {
            let RoleRunBoundNode::Directory(child) = open_role_run_child(&directory, expected)
                .map_err(|_| RoleRunInvocationError::InvalidPath)?
            else {
                return Err(RoleRunInvocationError::InvalidPath);
            };
            directory = child;
        } else if role_run_entry_is_symlink(&directory, expected)? {
            return Err(RoleRunInvocationError::InvalidPath);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn role_run_entry_exists_nofollow(
    directory: &std::fs::File,
    name: &str,
) -> Result<bool, RoleRunInvocationError> {
    use std::os::fd::AsRawFd;

    let name = std::ffi::CString::new(name).map_err(|_| RoleRunInvocationError::InvalidPath)?;
    // SAFETY: the directory descriptor is live, the name is NUL-terminated,
    // and the stat buffer is writable. No intermediate path is reopened.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        return Ok(true);
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ENOENT) => Ok(false),
        _ => Err(RoleRunInvocationError::InvalidPath),
    }
}

#[cfg(unix)]
fn role_run_entry_is_symlink(
    directory: &std::fs::File,
    name: &str,
) -> Result<bool, RoleRunInvocationError> {
    use std::os::fd::AsRawFd;

    let name = std::ffi::CString::new(name).map_err(|_| RoleRunInvocationError::InvalidPath)?;
    // SAFETY: the directory descriptor is live, the name is NUL-terminated,
    // and the stat buffer is writable. The no-follow flag inspects the final
    // node itself rather than a swapped target.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(RoleRunInvocationError::InvalidPath);
    }
    Ok(stat.st_mode & libc::S_IFMT == libc::S_IFLNK)
}

#[cfg(not(unix))]
fn require_exact_existing_path_spelling(
    _workspace: &Path,
    _relative: &Path,
) -> Result<(), RoleRunInvocationError> {
    Err(RoleRunInvocationError::InvalidPath)
}

fn is_private_role_run_path_name(name: &str) -> bool {
    name.eq_ignore_ascii_case(".git") || name.eq_ignore_ascii_case(".orchestrator")
}

fn skip_search_directory(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    name.starts_with('.')
        || matches!(
            name.as_ref(),
            "target"
                | "node_modules"
                | "dist"
                | "build"
                | "__pycache__"
                | ".venv"
                | "venv"
                | ".cache"
                | ".next"
                | ".nuxt"
        )
}

/// Execute a role-run read through a workspace-directory descriptor.
///
/// The ordinary interactive tools intentionally retain their existing path
/// behavior. Role-run calls this boundary only after broker approval and an
/// exact process grant. Every component is opened with `O_NOFOLLOW`; regular
/// files must have exactly one link and are read from the descriptor whose
/// inode was checked. Directory traversal is descriptor-relative and bounded.
pub async fn execute_role_run_read(
    workspace: &Path,
    expected_workspace: RoleRunWorkspaceIdentity,
    cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
    tool: &str,
    prepared_arguments: &serde_json::Value,
) -> crate::agent::registry::ToolExecutionResult {
    let workspace = workspace.to_path_buf();
    let tool = tool.to_string();
    let arguments = prepared_arguments.clone();
    match tokio::task::spawn_blocking(move || {
        execute_role_run_read_blocking(
            &workspace,
            expected_workspace,
            &cancellation,
            &tool,
            &arguments,
        )
    })
    .await
    {
        Ok(Ok(output)) => crate::agent::registry::ToolExecutionResult::Success(output),
        Ok(Err(())) | Err(_) => crate::agent::registry::ToolExecutionResult::Failure(
            "role-run read was rejected by the workspace boundary".into(),
        ),
    }
}

#[cfg(unix)]
struct RoleRunBoundFile {
    file: std::fs::File,
    metadata: std::fs::Metadata,
}

#[cfg(unix)]
enum RoleRunBoundNode {
    File(RoleRunBoundFile),
    Directory(std::fs::File),
}

#[cfg(unix)]
#[derive(Clone, Copy)]
enum RoleRunPrivatePathPolicy {
    Deny,
    OrchestratorSchema,
}

#[cfg(unix)]
struct RoleRunDirectoryStream(*mut libc::DIR);

#[cfg(unix)]
impl Drop for RoleRunDirectoryStream {
    fn drop(&mut self) {
        // SAFETY: the stream is created by `fdopendir` and owned exclusively
        // by this guard. `closedir` also closes its transferred descriptor.
        unsafe { libc::closedir(self.0) };
    }
}

#[cfg(unix)]
fn execute_role_run_read_blocking(
    workspace: &Path,
    expected_workspace: RoleRunWorkspaceIdentity,
    cancellation: &std::sync::atomic::AtomicBool,
    tool: &str,
    arguments: &serde_json::Value,
) -> Result<String, ()> {
    if role_run_is_cancelled(cancellation) {
        return Err(());
    }
    if !matches!(
        tool,
        "read_file" | "list_dir" | "search_files" | "glob" | "read_todos"
    ) {
        return Err(());
    }
    let object = arguments.as_object().ok_or(())?;
    let path = object
        .get("path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .ok_or(())?;
    let relative = role_run_relative_components(workspace, &path)?;
    let node = open_role_run_bound_node(workspace, expected_workspace, &relative)?;

    match tool {
        "read_file" => {
            let RoleRunBoundNode::File(file) = node else {
                return Err(());
            };
            if role_run_is_cancelled(cancellation) {
                return Err(());
            }
            let bytes =
                read_role_run_bound_file_cancellable(file, MAX_ROLE_RUN_FILE_BYTES, cancellation)?;
            if role_run_is_cancelled(cancellation) {
                return Err(());
            }
            let contents = std::str::from_utf8(&bytes).map_err(|_| ())?;
            let start = object
                .get("start_line")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(1);
            let end = object
                .get("end_line")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_else(|| start.saturating_add(199));
            role_run_read_file_output(contents, &path, start, end)
        }
        "list_dir" => {
            let RoleRunBoundNode::Directory(directory) = node else {
                return Err(());
            };
            let mut items = Vec::new();
            for name in
                read_role_run_directory_names(&directory, MAX_ROLE_RUN_LIST_ENTRIES, cancellation)?
            {
                if role_run_is_cancelled(cancellation) {
                    return Err(());
                }
                if is_private_role_run_path_name(&name) {
                    continue;
                }
                if items.len() >= MAX_ROLE_RUN_LIST_ENTRIES {
                    return Err(());
                }
                match open_role_run_child(&directory, &name)? {
                    RoleRunBoundNode::File(file) => {
                        ensure_role_run_bound_file(&file)?;
                        items.push(name);
                    }
                    RoleRunBoundNode::Directory(_) => items.push(format!("{name}/")),
                }
            }
            items.sort();
            Ok(truncate_role_run_output(
                &items.join("\n"),
                MAX_ROLE_RUN_SMALL_RESULT_BYTES,
            ))
        }
        "search_files" => {
            let pattern = object
                .get("pattern")
                .and_then(serde_json::Value::as_str)
                .ok_or(())?;
            let expression = regex::RegexBuilder::new(pattern)
                .size_limit(4 * 1024 * 1024)
                .dfa_size_limit(2 * 1024 * 1024)
                .build()
                .map_err(|_| ())?;
            execute_role_run_recursive_text_search(
                workspace,
                &relative,
                node,
                &expression,
                100,
                "No matches found",
                cancellation,
            )
        }
        "read_todos" => {
            let expression = regex::Regex::new(r"(?i)(TODO|FIXME|HACK|NOTE|XXX|BUG|OPTIMIZE)")
                .map_err(|_| ())?;
            execute_role_run_recursive_text_search(
                workspace,
                &relative,
                node,
                &expression,
                50,
                "No TODO/FIXME/HACK comments found",
                cancellation,
            )
        }
        "glob" => {
            let RoleRunBoundNode::Directory(directory) = node else {
                return Err(());
            };
            let pattern = object
                .get("pattern")
                .and_then(serde_json::Value::as_str)
                .and_then(|pattern| glob::Pattern::new(pattern).ok())
                .ok_or(())?;
            let options = glob::MatchOptions {
                case_sensitive: true,
                require_literal_separator: true,
                require_literal_leading_dot: true,
            };
            let matches = std::cell::RefCell::new(Vec::new());
            walk_role_run_bound_tree(
                directory,
                relative.clone(),
                cancellation,
                |_| true,
                |entry, directory| {
                    let candidate = entry.strip_prefix(&relative).map_err(|_| ())?;
                    if pattern.matches_path_with(candidate, options) {
                        matches
                            .borrow_mut()
                            .push(format!("{}/", workspace.join(entry).display()));
                    }
                    Ok(directory)
                },
                |entry, file| {
                    ensure_role_run_bound_file(&file)?;
                    let candidate = entry.strip_prefix(&relative).map_err(|_| ())?;
                    if pattern.matches_path_with(candidate, options) {
                        matches
                            .borrow_mut()
                            .push(workspace.join(entry).to_string_lossy().into_owned());
                    }
                    Ok(())
                },
            )?;
            let mut matches = matches.into_inner();
            matches.sort();
            matches.truncate(100);
            if matches.is_empty() {
                Ok("No files matched".into())
            } else {
                Ok(truncate_role_run_output(
                    &matches.join("\n"),
                    MAX_ROLE_RUN_SMALL_RESULT_BYTES,
                ))
            }
        }
        _ => Err(()),
    }
}

#[cfg(not(unix))]
fn execute_role_run_read_blocking(
    _workspace: &Path,
    _expected_workspace: RoleRunWorkspaceIdentity,
    _cancellation: &std::sync::atomic::AtomicBool,
    _tool: &str,
    _arguments: &serde_json::Value,
) -> Result<String, ()> {
    Err(())
}

fn role_run_is_cancelled(cancellation: &std::sync::atomic::AtomicBool) -> bool {
    cancellation.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(unix)]
fn role_run_relative_components(workspace: &Path, path: &Path) -> Result<PathBuf, ()> {
    if !workspace.is_absolute() || !path.is_absolute() || path == workspace {
        if path == workspace {
            return Ok(PathBuf::new());
        }
        return Err(());
    }
    let relative = path.strip_prefix(workspace).map_err(|_| ())?;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(());
        };
        let name = name.to_str().ok_or(())?;
        if name.chars().any(char::is_control) || is_private_role_run_path_name(name) {
            return Err(());
        }
    }
    Ok(relative.to_path_buf())
}

#[cfg(unix)]
fn open_role_run_bound_node(
    workspace: &Path,
    expected_workspace: RoleRunWorkspaceIdentity,
    relative: &Path,
) -> Result<RoleRunBoundNode, ()> {
    open_role_run_bound_node_with_policy(
        workspace,
        expected_workspace,
        relative,
        RoleRunPrivatePathPolicy::Deny,
    )
}

#[cfg(unix)]
fn open_role_run_bound_node_with_policy(
    workspace: &Path,
    expected_workspace: RoleRunWorkspaceIdentity,
    relative: &Path,
    private_path_policy: RoleRunPrivatePathPolicy,
) -> Result<RoleRunBoundNode, ()> {
    let mut current = open_role_run_workspace_root(workspace, expected_workspace)?;
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty() {
        return Ok(RoleRunBoundNode::Directory(current));
    }
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(());
        };
        let name = name.to_str().ok_or(())?;
        let allowed_orchestrator_schema_component = matches!(
            private_path_policy,
            RoleRunPrivatePathPolicy::OrchestratorSchema
        ) && index == 0
            && name == ".orchestrator"
            && matches!(
                components.get(1),
                Some(Component::Normal(schema)) if schema == &std::ffi::OsStr::new("schemas")
            );
        if name.chars().any(char::is_control)
            || (is_private_role_run_path_name(name) && !allowed_orchestrator_schema_component)
        {
            return Err(());
        }
        let child = open_role_run_child(&current, name)?;
        if index + 1 == components.len() {
            return Ok(child);
        }
        let RoleRunBoundNode::Directory(directory) = child else {
            return Err(());
        };
        current = directory;
    }
    Err(())
}

#[cfg(unix)]
fn open_role_run_workspace_root(
    workspace: &Path,
    expected_workspace: RoleRunWorkspaceIdentity,
) -> Result<std::fs::File, ()> {
    use std::os::fd::FromRawFd;
    use std::os::unix::ffi::OsStrExt;

    let workspace_name =
        std::ffi::CString::new(workspace.as_os_str().as_bytes()).map_err(|_| ())?;
    let before = std::fs::symlink_metadata(workspace).map_err(|_| ())?;
    if before.file_type().is_symlink() || !before.is_dir() {
        return Err(());
    }
    // SAFETY: `workspace_name` is NUL-terminated and ownership of a successful
    // descriptor is transferred to `File` exactly once.
    let root_fd = unsafe {
        libc::open(
            workspace_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if root_fd < 0 {
        return Err(());
    }
    // SAFETY: `open` returned one owned descriptor.
    let root = unsafe { std::fs::File::from_raw_fd(root_fd) };
    let opened = root.metadata().map_err(|_| ())?;
    if !opened.is_dir()
        || !same_role_run_file(&before, &opened)
        || !role_run_workspace_metadata_matches(&opened, expected_workspace)
    {
        return Err(());
    }
    Ok(root)
}

#[cfg(unix)]
fn role_run_workspace_metadata_matches(
    metadata: &std::fs::Metadata,
    expected: RoleRunWorkspaceIdentity,
) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.dev() == expected.device && metadata.ino() == expected.inode
}

#[cfg(unix)]
fn open_role_run_child(parent: &std::fs::File, name: &str) -> Result<RoleRunBoundNode, ()> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::MetadataExt;

    if name.is_empty()
        || matches!(name, "." | "..")
        || name.contains('/')
        || name.chars().any(char::is_control)
    {
        return Err(());
    }
    let name = std::ffi::CString::new(name).map_err(|_| ())?;
    // O_NONBLOCK prevents a raced special file from hanging before fstat can
    // reject it. O_NOFOLLOW rejects a raced symlink.
    // SAFETY: the parent descriptor is live, the name is NUL-terminated, and
    // ownership of a successful descriptor is transferred exactly once.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(());
    }
    // SAFETY: `openat` returned one owned descriptor.
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    let metadata = file.metadata().map_err(|_| ())?;
    let parent_metadata = parent.metadata().map_err(|_| ())?;
    if metadata.dev() != parent_metadata.dev() {
        return Err(());
    }
    if metadata.is_file() {
        if metadata.nlink() != 1 || metadata.len() > MAX_ROLE_RUN_FILE_BYTES {
            return Err(());
        }
        Ok(RoleRunBoundNode::File(RoleRunBoundFile { file, metadata }))
    } else if metadata.is_dir() {
        Ok(RoleRunBoundNode::Directory(file))
    } else {
        Err(())
    }
}

#[cfg(unix)]
fn ensure_role_run_bound_file(file: &RoleRunBoundFile) -> Result<(), ()> {
    use std::os::unix::fs::MetadataExt;

    let current = file.file.metadata().map_err(|_| ())?;
    if !current.is_file()
        || current.nlink() != 1
        || current.len() > MAX_ROLE_RUN_FILE_BYTES
        || !same_role_run_file(&file.metadata, &current)
    {
        return Err(());
    }
    Ok(())
}

#[cfg(unix)]
fn read_role_run_bound_file(file: RoleRunBoundFile, limit: u64) -> Result<Vec<u8>, ()> {
    read_role_run_bound_file_inner(file, limit, None)
}

#[cfg(unix)]
fn read_role_run_bound_file_cancellable(
    file: RoleRunBoundFile,
    limit: u64,
    cancellation: &std::sync::atomic::AtomicBool,
) -> Result<Vec<u8>, ()> {
    read_role_run_bound_file_inner(file, limit, Some(cancellation))
}

#[cfg(unix)]
fn read_role_run_bound_file_inner(
    mut file: RoleRunBoundFile,
    limit: u64,
    cancellation: Option<&std::sync::atomic::AtomicBool>,
) -> Result<Vec<u8>, ()> {
    ensure_role_run_bound_file(&file)?;
    if file.metadata.len() > limit {
        return Err(());
    }
    let mut bytes = Vec::with_capacity(usize::try_from(file.metadata.len()).unwrap_or(0));
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        if cancellation.is_some_and(role_run_is_cancelled) {
            return Err(());
        }
        let read = file.file.read(&mut buffer).map_err(|_| ())?;
        if read == 0 {
            break;
        }
        if bytes.len().saturating_add(read) as u64 > limit {
            return Err(());
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    if bytes.len() as u64 > limit {
        return Err(());
    }
    ensure_role_run_bound_file(&file)?;
    Ok(bytes)
}

#[cfg(unix)]
fn read_role_run_directory_names(
    directory: &std::fs::File,
    entry_limit: usize,
    cancellation: &std::sync::atomic::AtomicBool,
) -> Result<Vec<String>, ()> {
    use std::ffi::CStr;
    use std::os::fd::AsRawFd;

    let dot = c".";
    // A fresh open file description avoids sharing a directory offset with
    // the descriptor retained by the traversal stack.
    // SAFETY: the directory descriptor is live and `dot` is NUL-terminated.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            dot.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if descriptor < 0 {
        return Err(());
    }
    // SAFETY: ownership of `descriptor` transfers to the stream on success.
    let stream = unsafe { libc::fdopendir(descriptor) };
    if stream.is_null() {
        // SAFETY: fdopendir failed, so this call still owns the descriptor.
        unsafe { libc::close(descriptor) };
        return Err(());
    }
    let stream = RoleRunDirectoryStream(stream);
    let mut names = Vec::new();
    let mut name_bytes = 0usize;
    loop {
        if role_run_is_cancelled(cancellation) {
            return Err(());
        }
        clear_role_run_errno();
        // SAFETY: the stream is live and accessed by only this thread.
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            if role_run_errno() != 0 {
                return Err(());
            }
            break;
        }
        // SAFETY: readdir returned a live dirent whose d_name is NUL-terminated.
        let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if matches!(bytes, b"." | b"..") {
            continue;
        }
        let name = std::str::from_utf8(bytes).map_err(|_| ())?.to_string();
        if name.contains('/') || name.chars().any(char::is_control) {
            return Err(());
        }
        if names.len() >= entry_limit {
            return Err(());
        }
        name_bytes = name_bytes.checked_add(name.len()).ok_or(())?;
        if name_bytes > MAX_ROLE_RUN_DIRECTORY_NAMES_BYTES {
            return Err(());
        }
        names.push(name);
    }
    names.sort();
    Ok(names)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn clear_role_run_errno() {
    // SAFETY: libc exposes this thread's errno location.
    unsafe { *libc::__errno_location() = 0 };
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn role_run_errno() -> libc::c_int {
    // SAFETY: libc exposes this thread's errno location.
    unsafe { *libc::__errno_location() }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn clear_role_run_errno() {
    // SAFETY: libc exposes this thread's errno location.
    unsafe { *libc::__error() = 0 };
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn role_run_errno() -> libc::c_int {
    // SAFETY: libc exposes this thread's errno location.
    unsafe { *libc::__error() }
}

#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd"
    ))
))]
compile_error!("role-run descriptor traversal needs a supported thread-local errno API");

#[cfg(unix)]
fn walk_role_run_bound_tree<Recurse, OnDirectory, OnFile>(
    root: std::fs::File,
    root_relative: PathBuf,
    cancellation: &std::sync::atomic::AtomicBool,
    mut recurse: Recurse,
    mut on_directory: OnDirectory,
    mut on_file: OnFile,
) -> Result<(), ()>
where
    Recurse: FnMut(&str) -> bool,
    OnDirectory: FnMut(&Path, bool) -> Result<bool, ()>,
    OnFile: FnMut(&Path, RoleRunBoundFile) -> Result<(), ()>,
{
    let mut entries_seen = 0usize;
    let mut bytes_seen = 0u64;
    let mut directories = vec![(root, root_relative, 0usize)];
    while let Some((directory, logical, depth)) = directories.pop() {
        if role_run_is_cancelled(cancellation) {
            return Err(());
        }
        let remaining = MAX_ROLE_RUN_TREE_ENTRIES.saturating_sub(entries_seen);
        for name in read_role_run_directory_names(&directory, remaining, cancellation)? {
            if role_run_is_cancelled(cancellation) {
                return Err(());
            }
            entries_seen = entries_seen.checked_add(1).ok_or(())?;
            if entries_seen > MAX_ROLE_RUN_TREE_ENTRIES {
                return Err(());
            }
            if is_private_role_run_path_name(&name) {
                continue;
            }
            let child_path = logical.join(&name);
            match open_role_run_child(&directory, &name)? {
                RoleRunBoundNode::File(file) => {
                    bytes_seen = bytes_seen.checked_add(file.metadata.len()).ok_or(())?;
                    if bytes_seen > MAX_ROLE_RUN_TREE_BYTES {
                        return Err(());
                    }
                    on_file(&child_path, file)?;
                }
                RoleRunBoundNode::Directory(child) => {
                    let should_recurse = recurse(&name);
                    let should_recurse = on_directory(&child_path, should_recurse)?;
                    if should_recurse {
                        let next_depth = depth.checked_add(1).ok_or(())?;
                        if next_depth > MAX_ROLE_RUN_TREE_DEPTH {
                            return Err(());
                        }
                        directories.push((child, child_path, next_depth));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn execute_role_run_recursive_text_search(
    workspace: &Path,
    root_relative: &Path,
    node: RoleRunBoundNode,
    expression: &regex::Regex,
    match_limit: usize,
    empty_message: &str,
    cancellation: &std::sync::atomic::AtomicBool,
) -> Result<String, ()> {
    let mut results = Vec::new();
    let mut inspect = |relative: &Path, file: RoleRunBoundFile| -> Result<(), ()> {
        if role_run_is_cancelled(cancellation) {
            return Err(());
        }
        if role_run_hidden_or_binary_file(relative) || results.len() >= match_limit {
            ensure_role_run_bound_file(&file)?;
            return Ok(());
        }
        let bytes =
            read_role_run_bound_file_cancellable(file, MAX_ROLE_RUN_FILE_BYTES, cancellation)?;
        let Ok(contents) = std::str::from_utf8(&bytes) else {
            return Ok(());
        };
        let displayed = workspace.join(relative).to_string_lossy().into_owned();
        for (index, line) in contents.lines().enumerate() {
            if expression.is_match(line) {
                let line = truncate_role_run_output(line.trim(), 1024);
                results.push(format!("{displayed}:{}: {line}", index + 1));
                if results.len() >= match_limit {
                    break;
                }
            }
        }
        Ok(())
    };

    match node {
        RoleRunBoundNode::File(file) => inspect(root_relative, file)?,
        RoleRunBoundNode::Directory(directory) => walk_role_run_bound_tree(
            directory,
            root_relative.to_path_buf(),
            cancellation,
            |name| !skip_search_directory(std::ffi::OsStr::new(name)),
            |_, should_recurse| Ok(should_recurse),
            &mut inspect,
        )?,
    }
    if results.is_empty() {
        Ok(empty_message.into())
    } else {
        Ok(truncate_role_run_output(
            &results.join("\n"),
            MAX_ROLE_RUN_SMALL_RESULT_BYTES,
        ))
    }
}

#[cfg(unix)]
fn role_run_hidden_or_binary_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default();
    name.starts_with('.')
        || [".exe", ".bin", ".o", ".so", ".dylib", ".dll"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
}

#[cfg(unix)]
fn role_run_read_file_output(
    contents: &str,
    path: &Path,
    start: usize,
    end: usize,
) -> Result<String, ()> {
    if start == 0 || end < start || end.saturating_sub(start).saturating_add(1) > 500 {
        return Err(());
    }
    let lines = contents.lines().collect::<Vec<_>>();
    let total_lines = lines.len();
    let shown_start = start.min(total_lines.saturating_add(1));
    let shown_end = end.min(total_lines);
    let mut selected = if shown_start <= shown_end {
        lines[shown_start - 1..shown_end].join("\n")
    } else {
        String::new()
    };
    if !selected.is_empty() {
        selected.push('\n');
    }
    let showing = if shown_start <= shown_end {
        format!("Showing lines {shown_start}-{shown_end} of {total_lines}")
    } else {
        format!("Showing no lines (file has {total_lines})")
    };
    let before = start.saturating_sub(1).min(total_lines);
    let after = total_lines.saturating_sub(shown_end);
    let mut footer = format!(
        "\n{showing} · {} bytes total\nOmitted lines: {before} before, {after} after",
        contents.len()
    );
    if shown_end < total_lines {
        let next_start = shown_end.saturating_add(1).max(1);
        let next_end = next_start.saturating_add(199).min(total_lines);
        footer.push_str(&format!(
            "\nNext range: {}",
            serde_json::json!({
                "path": path,
                "start_line": next_start,
                "end_line": next_end,
            })
        ));
    }
    let body_budget = MAX_ROLE_RUN_READ_RESULT_BYTES.saturating_sub(footer.len());
    let body = truncate_role_run_output(&selected, body_budget);
    Ok(format!("{body}{footer}"))
}

fn truncate_role_run_output(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    const MARKER: &str = "\n[role-run output truncated]";
    let mut end = max_bytes.saturating_sub(MARKER.len()).min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{MARKER}", &value[..end])
}

#[derive(Clone, Copy)]
struct RoleRunPatchRange {
    start: usize,
    count: usize,
}

fn parse_role_run_patch_range(token: &str, prefix: char) -> Result<RoleRunPatchRange, ()> {
    let raw = token.strip_prefix(prefix).ok_or(())?;
    let (start, count) = match raw.split_once(',') {
        Some((start, count)) => (
            start.parse::<usize>().map_err(|_| ())?,
            count.parse::<usize>().map_err(|_| ())?,
        ),
        None => (raw.parse::<usize>().map_err(|_| ())?, 1),
    };
    if count > 0 && start == 0 {
        return Err(());
    }
    let start = if count == 0 {
        start
    } else {
        start.checked_sub(1).ok_or(())?
    };
    Ok(RoleRunPatchRange { start, count })
}

fn parse_role_run_patch_header(line: &str) -> Result<(RoleRunPatchRange, RoleRunPatchRange), ()> {
    let ranges = line
        .strip_prefix("@@ ")
        .and_then(|body| body.split_once(" @@").map(|(ranges, _)| ranges))
        .ok_or(())?;
    let mut ranges = ranges.split_whitespace();
    let old = parse_role_run_patch_range(ranges.next().ok_or(())?, '-')?;
    let new = parse_role_run_patch_range(ranges.next().ok_or(())?, '+')?;
    if ranges.next().is_some() {
        return Err(());
    }
    Ok((old, new))
}

fn append_role_run_patch_line(
    output: &mut String,
    output_lines: &mut usize,
    line: &str,
) -> Result<(), ()> {
    let separator = usize::from(*output_lines > 0);
    let next_len = output
        .len()
        .checked_add(separator)
        .and_then(|len| len.checked_add(line.len()))
        .filter(|len| *len <= crate::undo::MAX_UNDO_FILE_BYTES)
        .ok_or(())?;
    output.reserve(next_len.saturating_sub(output.len()));
    if separator != 0 {
        output.push('\n');
    }
    output.push_str(line);
    *output_lines = output_lines.checked_add(1).ok_or(())?;
    Ok(())
}

fn apply_role_run_unified_patch(
    original: &str,
    patch: &str,
    cancellation: &std::sync::atomic::AtomicBool,
) -> Result<String, ()> {
    if role_run_is_cancelled(cancellation) {
        return Err(());
    }
    let source = original.lines().collect::<Vec<_>>();
    if source.len() > MAX_ROLE_RUN_SOURCE_LINES {
        return Err(());
    }

    let mut patch_lines = patch.lines().peekable();
    let mut output = String::with_capacity(original.len().min(crate::undo::MAX_UNDO_FILE_BYTES));
    let mut output_lines = 0usize;
    let mut source_cursor = 0usize;
    let mut hunk_count = 0usize;
    let mut line_count = 0usize;

    while let Some(header) = patch_lines.next() {
        line_count = line_count.checked_add(1).ok_or(())?;
        if line_count > MAX_ROLE_RUN_PATCH_LINES || role_run_is_cancelled(cancellation) {
            return Err(());
        }
        let (old, new) = parse_role_run_patch_header(header)?;
        hunk_count = hunk_count.checked_add(1).ok_or(())?;
        if hunk_count > MAX_ROLE_RUN_PATCH_HUNKS
            || old.start < source_cursor
            || old.start > source.len()
        {
            return Err(());
        }

        for line in &source[source_cursor..old.start] {
            if role_run_is_cancelled(cancellation) {
                return Err(());
            }
            append_role_run_patch_line(&mut output, &mut output_lines, line)?;
        }
        source_cursor = old.start;
        // Positive ranges normalize to their zero-based first line. A
        // zero-count range stays at the cursor after its preceding line, so
        // both forms must equal the output constructed before this hunk.
        let expected_new_start = output_lines;
        if new.start != expected_new_start {
            return Err(());
        }

        let mut old_seen = 0usize;
        let mut new_seen = 0usize;
        while patch_lines
            .peek()
            .is_some_and(|line| !line.starts_with("@@ "))
        {
            let line = patch_lines.next().ok_or(())?;
            line_count = line_count.checked_add(1).ok_or(())?;
            if line_count > MAX_ROLE_RUN_PATCH_LINES || role_run_is_cancelled(cancellation) {
                return Err(());
            }
            if let Some(context) = line.strip_prefix(' ') {
                if source.get(source_cursor).copied() != Some(context) {
                    return Err(());
                }
                append_role_run_patch_line(&mut output, &mut output_lines, context)?;
                source_cursor = source_cursor.checked_add(1).ok_or(())?;
                old_seen = old_seen.checked_add(1).ok_or(())?;
                new_seen = new_seen.checked_add(1).ok_or(())?;
            } else if let Some(removed) = line.strip_prefix('-') {
                if source.get(source_cursor).copied() != Some(removed) {
                    return Err(());
                }
                source_cursor = source_cursor.checked_add(1).ok_or(())?;
                old_seen = old_seen.checked_add(1).ok_or(())?;
            } else if let Some(added) = line.strip_prefix('+') {
                append_role_run_patch_line(&mut output, &mut output_lines, added)?;
                new_seen = new_seen.checked_add(1).ok_or(())?;
            } else {
                return Err(());
            }
        }
        if old_seen != old.count || new_seen != new.count {
            return Err(());
        }
    }

    if hunk_count == 0 {
        return Err(());
    }
    for line in &source[source_cursor..] {
        if role_run_is_cancelled(cancellation) {
            return Err(());
        }
        append_role_run_patch_line(&mut output, &mut output_lines, line)?;
    }
    Ok(output)
}

/// Execute a role-run write through a separate mutation boundary.
///
/// The caller must invoke this only after the approval broker, persistent Deny
/// ceilings, and exact process grant have all accepted the prepared call. The
/// ordinary interactive file tools intentionally keep their established
/// behavior; this path is exclusively for role-run's stronger containment
/// contract.
pub async fn execute_role_run_mutation(
    workspace: &Path,
    expected_workspace: RoleRunWorkspaceIdentity,
    cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
    output_redactions: crate::headless::HeadlessOutputRedactions,
    tool: &str,
    prepared_arguments: &serde_json::Value,
) -> crate::agent::registry::ToolExecutionResult {
    let workspace = workspace.to_path_buf();
    let tool = tool.to_string();
    let arguments = prepared_arguments.clone();
    match tokio::task::spawn_blocking(move || {
        execute_role_run_mutation_blocking(
            &workspace,
            expected_workspace,
            &cancellation,
            &output_redactions,
            &tool,
            &arguments,
        )
    })
    .await
    {
        Ok(Ok(())) => crate::agent::registry::ToolExecutionResult::Success(
            "role-run file mutation completed".into(),
        ),
        Ok(Err(())) | Err(_) => crate::agent::registry::ToolExecutionResult::Failure(
            "role-run file mutation was rejected".into(),
        ),
    }
}

#[cfg(unix)]
fn execute_role_run_mutation_blocking(
    workspace: &Path,
    expected_workspace: RoleRunWorkspaceIdentity,
    cancellation: &std::sync::atomic::AtomicBool,
    output_redactions: &crate::headless::HeadlessOutputRedactions,
    tool: &str,
    arguments: &serde_json::Value,
) -> Result<(), ()> {
    use std::ffi::{CStr, CString};
    use std::fs::File;
    use std::io::{Read as _, Write as _};
    use std::os::fd::{AsRawFd, FromRawFd, RawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    #[derive(Clone, Copy, PartialEq, Eq)]
    struct Identity {
        device: u64,
        inode: u64,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum TargetSnapshot {
        Missing,
        Regular {
            identity: Identity,
            mode: libc::mode_t,
        },
    }

    fn identity(metadata: &std::fs::Metadata) -> Identity {
        Identity {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }

    fn c_string(value: &std::ffi::OsStr) -> Result<CString, ()> {
        CString::new(value.as_bytes()).map_err(|_| ())
    }

    fn open_directory(path: &Path) -> Result<File, ()> {
        let path = c_string(path.as_os_str())?;
        // SAFETY: `path` is NUL-terminated and the returned descriptor is
        // uniquely transferred into `File` on success.
        let fd = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(());
        }
        // SAFETY: successful `open` returned one owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn open_directory_at(parent: RawFd, name: &CStr) -> Result<File, ()> {
        // SAFETY: `name` is NUL-terminated, `parent` is a live directory fd,
        // and the returned descriptor is uniquely transferred to `File`.
        let fd = unsafe {
            libc::openat(
                parent,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(());
        }
        // SAFETY: successful `openat` returned one owned descriptor.
        let directory = unsafe { File::from_raw_fd(fd) };
        // SAFETY: both metadata buffers are initialized for fstat and both
        // descriptors are live for the duration of the calls.
        let mut parent_stat: libc::stat = unsafe { std::mem::zeroed() };
        let mut child_stat: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(parent, &mut parent_stat) } != 0
            || unsafe { libc::fstat(directory.as_raw_fd(), &mut child_stat) } != 0
            || parent_stat.st_dev != child_stat.st_dev
        {
            return Err(());
        }
        Ok(directory)
    }

    fn open_parent(
        workspace: &Path,
        expected_workspace: RoleRunWorkspaceIdentity,
        components: &[&std::ffi::OsStr],
    ) -> Result<File, ()> {
        let mut directory = open_directory(workspace)?;
        let root = directory.metadata().map_err(|_| ())?;
        if root.dev() != expected_workspace.device || root.ino() != expected_workspace.inode {
            return Err(());
        }
        for component in components {
            let component = c_string(component)?;
            directory = open_directory_at(directory.as_raw_fd(), &component)?;
        }
        Ok(directory)
    }

    fn stat_target(parent: RawFd, name: &CStr) -> Result<TargetSnapshot, ()> {
        // SAFETY: zeroed `stat` is an initialization buffer for `fstatat`.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `parent` is live, `name` is NUL-terminated, and `stat`
        // points to writable storage. AT_SYMLINK_NOFOLLOW is essential here.
        let result =
            unsafe { libc::fstatat(parent, name.as_ptr(), &mut stat, libc::AT_SYMLINK_NOFOLLOW) };
        if result != 0 {
            return (std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT))
                .then_some(TargetSnapshot::Missing)
                .ok_or(());
        }
        let mut parent_stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: parent is live and parent_stat is writable storage.
        if unsafe { libc::fstat(parent, &mut parent_stat) } != 0
            || stat.st_dev != parent_stat.st_dev
            || stat.st_mode & libc::S_IFMT != libc::S_IFREG
            || stat.st_nlink != 1
        {
            return Err(());
        }
        Ok(TargetSnapshot::Regular {
            identity: Identity {
                device: stat.st_dev as u64,
                inode: stat.st_ino as u64,
            },
            mode: stat.st_mode,
        })
    }

    fn read_existing(
        parent: RawFd,
        name: &CStr,
        expected: TargetSnapshot,
    ) -> Result<crate::undo::FileState, ()> {
        let TargetSnapshot::Regular {
            identity: expected_identity,
            ..
        } = expected
        else {
            return Ok(crate::undo::FileState::Missing);
        };
        // O_NONBLOCK prevents a swapped FIFO from hanging before fstat rejects
        // it; O_NOFOLLOW rejects a swapped symlink.
        // SAFETY: arguments meet `openat`'s requirements and ownership of a
        // successful descriptor is transferred to `File` exactly once.
        let fd = unsafe {
            libc::openat(
                parent,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            )
        };
        if fd < 0 {
            return Err(());
        }
        // SAFETY: successful `openat` returned one owned descriptor.
        let mut file = unsafe { File::from_raw_fd(fd) };
        let metadata = file.metadata().map_err(|_| ())?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || identity(&metadata) != expected_identity
            || metadata.len() > crate::undo::MAX_UNDO_FILE_BYTES as u64
        {
            return Err(());
        }
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take(crate::undo::MAX_UNDO_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ())?;
        if bytes.len() > crate::undo::MAX_UNDO_FILE_BYTES {
            return Err(());
        }
        let content = String::from_utf8(bytes).map_err(|_| ())?;
        crate::undo::FileState::utf8(content).map_err(|_| ())
    }

    fn create_temp(parent: RawFd) -> Result<(CString, File), ()> {
        for _ in 0..8 {
            let name = CString::new(format!(".ca-role-run-{}.tmp", uuid::Uuid::new_v4()))
                .map_err(|_| ())?;
            // SAFETY: `parent` is live and `name` is NUL-terminated. O_EXCL
            // gives the new entry a unique inode owned by this call.
            let fd = unsafe {
                libc::openat(
                    parent,
                    name.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_CLOEXEC
                        | libc::O_NOFOLLOW,
                    0o600,
                )
            };
            if fd >= 0 {
                // SAFETY: successful `openat` returned one owned descriptor.
                return Ok((name, unsafe { File::from_raw_fd(fd) }));
            }
            if std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
                return Err(());
            }
        }
        Err(())
    }

    fn unlink_temp(parent: RawFd, name: &CStr) {
        // SAFETY: `parent` is live and `name` is NUL-terminated. Failure is
        // intentionally ignored during best-effort cleanup.
        let _ = unsafe { libc::unlinkat(parent, name.as_ptr(), 0) };
    }

    if role_run_is_cancelled(cancellation) || !WRITE_TOOLS.contains(&tool) {
        return Err(());
    }
    let path = arguments
        .get("path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.starts_with(workspace))
        .ok_or(())?;
    let relative = path.strip_prefix(workspace).map_err(|_| ())?;
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty()
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
        || components.iter().any(|component| {
            let Component::Normal(component) = component else {
                return true;
            };
            component
                .to_str()
                .is_some_and(is_private_role_run_path_name)
        })
        || components.first().is_some_and(|component| {
            let Component::Normal(component) = component else {
                return true;
            };
            component.to_str().is_some_and(|component| {
                component.eq_ignore_ascii_case("oracles")
                    || component.eq_ignore_ascii_case("reference")
            })
        })
    {
        return Err(());
    }
    let names = components
        .iter()
        .map(|component| match component {
            Component::Normal(name) => Ok(*name),
            _ => Err(()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (target_name, parent_components) = names.split_last().ok_or(())?;
    let target_name = c_string(target_name)?;
    let parent = open_parent(workspace, expected_workspace, parent_components)?;
    let parent_identity = identity(&parent.metadata().map_err(|_| ())?);
    let snapshot = stat_target(parent.as_raw_fd(), &target_name)?;
    let before = read_existing(parent.as_raw_fd(), &target_name, snapshot)?;
    if role_run_is_cancelled(cancellation) {
        return Err(());
    }
    let after_content = match tool {
        "write_file" => arguments
            .get("content")
            .and_then(serde_json::Value::as_str)
            .ok_or(())?
            .to_string(),
        "apply_patch" => {
            let crate::undo::FileState::Utf8(original) = &before else {
                return Err(());
            };
            let patch = arguments
                .get("patch")
                .and_then(serde_json::Value::as_str)
                .ok_or(())?;
            apply_role_run_unified_patch(original, patch, cancellation)?
        }
        _ => return Err(()),
    };
    if after_content.len() > crate::undo::MAX_UNDO_FILE_BYTES
        || output_redactions.text_contains_secret(&after_content)
    {
        return Err(());
    }

    let (temp_name, mut temp) = create_temp(parent.as_raw_fd())?;
    let temp_identity = identity(&temp.metadata().map_err(|_| ())?);
    let result = (|| {
        if role_run_is_cancelled(cancellation) {
            return Err(());
        }
        for chunk in after_content.as_bytes().chunks(8 * 1024) {
            if role_run_is_cancelled(cancellation) {
                return Err(());
            }
            temp.write_all(chunk).map_err(|_| ())?;
        }
        if let TargetSnapshot::Regular { mode, .. } = snapshot {
            // Preserve ordinary executable/read permissions without carrying
            // set-id or sticky bits onto the replacement inode.
            // SAFETY: `temp` owns a live descriptor and fchmod does not follow
            // a path.
            if unsafe { libc::fchmod(temp.as_raw_fd(), mode & 0o777) } != 0 {
                return Err(());
            }
        }
        if role_run_is_cancelled(cancellation) {
            return Err(());
        }
        temp.sync_all().map_err(|_| ())?;
        if role_run_is_cancelled(cancellation) {
            return Err(());
        }

        // Re-open the exact parent chain from the canonical workspace and
        // compare its inode to the bound descriptor. A swapped symlink or
        // directory is rejected before the atomic rename.
        let current_parent = open_parent(workspace, expected_workspace, parent_components)?;
        if identity(&current_parent.metadata().map_err(|_| ())?) != parent_identity
            || stat_target(parent.as_raw_fd(), &target_name)? != snapshot
        {
            return Err(());
        }
        let TargetSnapshot::Regular {
            identity: current_temp_identity,
            ..
        } = stat_target(parent.as_raw_fd(), &temp_name)?
        else {
            return Err(());
        };
        if current_temp_identity != temp_identity
            || identity(&temp.metadata().map_err(|_| ())?) != temp_identity
            || role_run_is_cancelled(cancellation)
        {
            return Err(());
        }

        // SAFETY: both descriptors name the same bound directory and both
        // names are NUL-terminated. renameat replaces the directory entry; it
        // never follows a swapped target symlink or writes through a hardlink.
        if unsafe {
            libc::renameat(
                parent.as_raw_fd(),
                temp_name.as_ptr(),
                parent.as_raw_fd(),
                target_name.as_ptr(),
            )
        } != 0
        {
            return Err(());
        }
        parent.sync_all().map_err(|_| ())?;
        if role_run_is_cancelled(cancellation) {
            return Err(());
        }
        let replaced = stat_target(parent.as_raw_fd(), &target_name)?;
        if !matches!(
            replaced,
            TargetSnapshot::Regular { identity, .. } if identity == temp_identity
        ) {
            return Err(());
        }
        Ok(())
    })();
    if result.is_err() {
        unlink_temp(parent.as_raw_fd(), &temp_name);
        return Err(());
    }
    Ok(())
}

#[cfg(not(unix))]
fn execute_role_run_mutation_blocking(
    _workspace: &Path,
    _expected_workspace: RoleRunWorkspaceIdentity,
    _cancellation: &std::sync::atomic::AtomicBool,
    _output_redactions: &crate::headless::HeadlessOutputRedactions,
    _tool: &str,
    _arguments: &serde_json::Value,
) -> Result<(), ()> {
    // V1 fails closed on platforms where the no-follow, directory-fd boundary
    // is unavailable.
    Err(())
}

/// Execute the one role-run tool that would otherwise cross a process boundary.
///
/// The caller must run the general role-run invocation validator before the
/// approval broker, then call this function only after the exact grant and all
/// persistent Deny ceilings have been evaluated. This function revalidates the
/// prepared invocation, snapshots the regular file through an O_NOFOLLOW open,
/// and runs a fixed system Git in an owned process group. Dropping the future
/// hard-kills and synchronously reaps the direct child; the outer headless
/// cancellation select therefore remains the sole owner of process signals.
pub async fn execute_role_run_blame(
    workspace: &Path,
    expected_workspace: RoleRunWorkspaceIdentity,
    cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
    prepared_arguments: &serde_json::Value,
    timeout_secs: u64,
) -> crate::agent::registry::ToolExecutionResult {
    if role_run_is_cancelled(&cancellation) {
        return crate::agent::registry::ToolExecutionResult::Cancelled(
            "role-run blame cancelled".into(),
        );
    }
    let arguments = match validate_role_run_tool_invocation(workspace, "blame", prepared_arguments)
    {
        Ok(arguments) => arguments,
        Err(_) => {
            return crate::agent::registry::ToolExecutionResult::Failure(
                "role-run blame arguments are invalid".into(),
            )
        }
    };
    let Some(path) = arguments
        .get("path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
    else {
        return crate::agent::registry::ToolExecutionResult::Failure(
            "role-run blame arguments are invalid".into(),
        );
    };
    let relative = match path.strip_prefix(workspace) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative.to_path_buf(),
        _ => {
            return crate::agent::registry::ToolExecutionResult::Failure(
                "role-run blame path is invalid".into(),
            )
        }
    };
    let git = match trusted_role_run_git_binary() {
        Ok(git) => git,
        Err(_) => {
            return crate::agent::registry::ToolExecutionResult::Failure(
                "role-run blame is unavailable".into(),
            )
        }
    };
    #[cfg(not(unix))]
    return crate::agent::registry::ToolExecutionResult::Failure(
        "role-run blame is unavailable".into(),
    );
    #[cfg(unix)]
    let process_root = match open_role_run_workspace_root(workspace, expected_workspace) {
        Ok(root) => root,
        Err(_) => {
            return crate::agent::registry::ToolExecutionResult::Failure(
                "role-run blame workspace changed".into(),
            )
        }
    };
    #[cfg(unix)]
    let Some(git_entry) = role_run_workspace_git_repository(&process_root, workspace) else {
        return crate::agent::registry::ToolExecutionResult::Failure(
            "role-run blame requires a repository at the exact workspace".into(),
        );
    };
    #[cfg(unix)]
    let contents = match open_role_run_bound_node(workspace, expected_workspace, &relative)
        .and_then(|node| match node {
            RoleRunBoundNode::File(file) => read_role_run_bound_file(file, MAX_ROLE_RUN_FILE_BYTES),
            RoleRunBoundNode::Directory(_) => Err(()),
        }) {
        Ok(contents) => contents,
        Err(_) => {
            return crate::agent::registry::ToolExecutionResult::Failure(
                "role-run blame file could not be read safely".into(),
            )
        }
    };

    let mut command = tokio::process::Command::new(git);
    command
        .env_clear()
        .env("HOME", "/var/empty")
        .env("XDG_CONFIG_HOME", "/var/empty")
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_PROTOCOL_FROM_USER", "0")
        .env("GIT_ALLOW_PROTOCOL", "file")
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat")
        .env(
            "GIT_CEILING_DIRECTORIES",
            workspace.parent().unwrap_or(workspace),
        )
        .arg("--literal-pathspecs")
        .arg("--no-pager")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-c")
        .arg("credential.helper=")
        .arg("-c")
        .arg("pager.blame=false")
        .arg("-c")
        .arg("color.ui=false")
        .arg("-c")
        .arg("diff.external=")
        .arg("blame")
        .arg("--no-progress")
        .arg("--no-ext-diff")
        .arg("--no-textconv");
    if let (Some(start), Some(end)) = (
        arguments
            .get("start_line")
            .and_then(serde_json::Value::as_u64),
        arguments
            .get("end_line")
            .and_then(serde_json::Value::as_u64),
    ) {
        command.arg("-L").arg(format!("{start},{end}"));
    }
    command.arg("--contents").arg("-").arg("--").arg(relative);

    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;

        // The child changes directory through the descriptor captured from
        // the validated workspace inode. No absolute pathname is reopened
        // between validation and Git execution.
        unsafe {
            command.as_std_mut().pre_exec(move || {
                if libc::fchdir(process_root.as_raw_fd()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let mut current: libc::stat = std::mem::zeroed();
                if libc::fstatat(
                    process_root.as_raw_fd(),
                    c".git".as_ptr(),
                    &mut current,
                    libc::AT_SYMLINK_NOFOLLOW,
                ) != 0
                    || current.st_dev as u64 != git_entry.device
                    || current.st_ino as u64 != git_entry.inode
                    || u32::from(current.st_mode & libc::S_IFMT) != git_entry.file_type
                {
                    return Err(std::io::Error::from_raw_os_error(libc::ESTALE));
                }
                Ok(())
            });
        }
    }

    let timeout = Duration::from_secs(timeout_secs.clamp(1, ROLE_RUN_TOOL_TIMEOUT_SECONDS));
    let mut process = Box::pin(run_bounded_role_run_process(command, contents, timeout));
    let process_result = tokio::select! {
        result = &mut process => result,
        _ = wait_for_role_run_cancellation(cancellation.clone()) => {
            // Dropping the process future drops its process-group guard, which
            // synchronously kills and reaps the direct child before returning.
            drop(process);
            return crate::agent::registry::ToolExecutionResult::Cancelled(
                "role-run blame cancelled".into(),
            );
        }
    };
    let output = match process_result {
        Ok(output) => output,
        Err(RoleRunProcessError::TimedOut) => {
            return crate::agent::registry::ToolExecutionResult::Cancelled(
                "role-run blame timed out".into(),
            )
        }
        Err(_) => {
            return crate::agent::registry::ToolExecutionResult::Failure(
                "role-run blame could not be executed safely".into(),
            )
        }
    };
    if !output.status.success() || !output.stdin_complete {
        return crate::agent::registry::ToolExecutionResult::Failure(
            "role-run blame failed".into(),
        );
    }
    if output.stdout_overflow || output.stderr_overflow {
        return crate::agent::registry::ToolExecutionResult::Failure(
            "role-run blame output exceeded its safety bound".into(),
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    crate::agent::registry::ToolExecutionResult::Success(if stdout.is_empty() {
        "No blame output".into()
    } else {
        stdout
    })
}

async fn wait_for_role_run_cancellation(
    cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    while !role_run_is_cancelled(&cancellation) {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(unix)]
#[derive(Clone, Copy)]
struct RoleRunGitEntryIdentity {
    device: u64,
    inode: u64,
    file_type: u32,
}

#[cfg(unix)]
fn role_run_workspace_git_repository(
    workspace_root: &std::fs::File,
    workspace: &Path,
) -> Option<RoleRunGitEntryIdentity> {
    use std::os::unix::fs::MetadataExt;

    let dot_git = open_role_run_child(workspace_root, ".git").ok()?;
    let (identity, pointer) = match dot_git {
        RoleRunBoundNode::Directory(directory) => {
            let metadata = directory.metadata().ok()?;
            return Some(RoleRunGitEntryIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
                file_type: metadata.mode() & u32::from(libc::S_IFMT),
            });
        }
        RoleRunBoundNode::File(file) => {
            let metadata = file.metadata.clone();
            let bytes = read_role_run_bound_file(file, 4 * 1024).ok()?;
            (
                RoleRunGitEntryIdentity {
                    device: metadata.dev(),
                    inode: metadata.ino(),
                    file_type: metadata.mode() & u32::from(libc::S_IFMT),
                },
                bytes,
            )
        }
    };
    let pointer = std::str::from_utf8(&pointer)
        .ok()?
        .trim_end_matches(['\r', '\n']);
    if pointer.lines().count() != 1 || pointer.chars().any(char::is_control) {
        return None;
    }
    let raw_admin = Path::new(pointer.strip_prefix("gitdir: ")?);
    if raw_admin.as_os_str().is_empty() {
        return None;
    }
    let admin = normalize_role_run_absolute_path(if raw_admin.is_absolute() {
        raw_admin.to_path_buf()
    } else {
        workspace.join(raw_admin)
    })?;
    if admin.canonicalize().ok()? != admin {
        return None;
    }
    let admin_directory = open_role_run_absolute_directory(&admin).ok()?;
    let RoleRunBoundNode::File(backlink) = open_role_run_child(&admin_directory, "gitdir").ok()?
    else {
        return None;
    };
    let backlink = read_role_run_bound_file(backlink, 4 * 1024).ok()?;
    let backlink = std::str::from_utf8(&backlink)
        .ok()?
        .trim_end_matches(['\r', '\n']);
    if backlink.is_empty()
        || backlink.lines().count() != 1
        || backlink.chars().any(char::is_control)
    {
        return None;
    }
    let backlink = Path::new(backlink);
    let backlink = normalize_role_run_absolute_path(if backlink.is_absolute() {
        backlink.to_path_buf()
    } else {
        admin.join(backlink)
    })?;
    (backlink == workspace.join(".git")).then_some(identity)
}

#[cfg(unix)]
fn normalize_role_run_absolute_path(path: PathBuf) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir => normalized = PathBuf::from("/"),
            Component::Normal(component) => normalized.push(component),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Prefix(_) => return None,
        }
    }
    Some(normalized)
}

#[cfg(unix)]
fn open_role_run_absolute_directory(path: &Path) -> Result<std::fs::File, ()> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;

    let root = c"/";
    // SAFETY: root is NUL-terminated and ownership transfers once.
    let descriptor = unsafe {
        libc::open(
            root.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if descriptor < 0 {
        return Err(());
    }
    // SAFETY: open returned one owned descriptor.
    let mut directory = unsafe { std::fs::File::from_raw_fd(descriptor) };
    for component in path.components() {
        let Component::Normal(component) = component else {
            if matches!(component, Component::RootDir) {
                continue;
            }
            return Err(());
        };
        let component = std::ffi::CString::new(component.as_bytes()).map_err(|_| ())?;
        // SAFETY: the parent is live, component is NUL-terminated, and a
        // successful descriptor transfers ownership once.
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                component.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if descriptor < 0 {
            return Err(());
        }
        // SAFETY: openat returned one owned descriptor.
        directory = unsafe { std::fs::File::from_raw_fd(descriptor) };
    }
    Ok(directory)
}

fn trusted_role_run_git_binary() -> Result<PathBuf, RoleRunProcessError> {
    for candidate in [Path::new("/usr/bin/git"), Path::new("/bin/git")] {
        let Ok(metadata) = std::fs::symlink_metadata(candidate) else {
            continue;
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        return Ok(candidate.to_path_buf());
    }
    Err(RoleRunProcessError::Unavailable)
}

#[cfg(unix)]
fn same_role_run_file(before: &std::fs::Metadata, opened: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    before.dev() == opened.dev() && before.ino() == opened.ino()
}

#[cfg(not(unix))]
fn same_role_run_file(before: &std::fs::Metadata, opened: &std::fs::Metadata) -> bool {
    before.len() == opened.len()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoleRunProcessError {
    Unavailable,
    Spawn,
    Io,
    TimedOut,
}

struct RoleRunProcessOutput {
    status: std::process::ExitStatus,
    stdin_complete: bool,
    stdout: Vec<u8>,
    stdout_overflow: bool,
    stderr_overflow: bool,
}

struct BoundedRoleRunPipe {
    bytes: Vec<u8>,
    overflow: bool,
}

async fn run_bounded_role_run_process(
    mut command: tokio::process::Command,
    stdin_bytes: Vec<u8>,
    timeout: Duration,
) -> Result<RoleRunProcessOutput, RoleRunProcessError> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let mut child = command.spawn().map_err(|_| RoleRunProcessError::Spawn)?;
    let process_group = child.id().and_then(|id| i32::try_from(id).ok());
    let child_pid = process_group;
    let mut guard = RoleRunProcessGroupGuard {
        process_group,
        child_pid,
    };
    let mut stdin = child.stdin.take().ok_or(RoleRunProcessError::Io)?;
    let stdout = child.stdout.take().ok_or(RoleRunProcessError::Io)?;
    let stderr = child.stderr.take().ok_or(RoleRunProcessError::Io)?;
    let stdin_task = tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(&stdin_bytes).await?;
        stdin.shutdown().await
    });
    let stdout_task = tokio::spawn(read_bounded_role_run_pipe(
        stdout,
        MAX_ROLE_RUN_BLAME_OUTPUT_BYTES,
    ));
    let stderr_task = tokio::spawn(read_bounded_role_run_pipe(
        stderr,
        MAX_ROLE_RUN_BLAME_ERROR_BYTES,
    ));

    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(_)) => {
            terminate_and_reap_role_run_child(&mut child, process_group).await;
            guard.disarm();
            abort_role_run_io_tasks(stdin_task, stdout_task, stderr_task).await;
            return Err(RoleRunProcessError::Io);
        }
        Err(_) => {
            terminate_and_reap_role_run_child(&mut child, process_group).await;
            guard.disarm();
            abort_role_run_io_tasks(stdin_task, stdout_task, stderr_task).await;
            return Err(RoleRunProcessError::TimedOut);
        }
    };
    terminate_role_run_process_group(process_group, true);
    guard.disarm();

    let stdin_complete = join_role_run_io_task(stdin_task)
        .await
        .is_ok_and(|result| result.is_ok());
    let stdout = join_role_run_io_task(stdout_task)
        .await
        .and_then(|result| result.map_err(|_| RoleRunProcessError::Io))?;
    let stderr = join_role_run_io_task(stderr_task)
        .await
        .and_then(|result| result.map_err(|_| RoleRunProcessError::Io))?;
    Ok(RoleRunProcessOutput {
        status,
        stdin_complete,
        stdout: stdout.bytes,
        stdout_overflow: stdout.overflow,
        stderr_overflow: stderr.overflow,
    })
}

async fn read_bounded_role_run_pipe(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    limit: usize,
) -> std::io::Result<BoundedRoleRunPipe> {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    let mut buffer = [0_u8; 8 * 1024];
    let mut overflow = false;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let retained = limit.saturating_sub(bytes.len()).min(read);
        bytes.extend_from_slice(&buffer[..retained]);
        overflow |= retained < read;
    }
    Ok(BoundedRoleRunPipe { bytes, overflow })
}

async fn join_role_run_io_task<T>(
    mut task: tokio::task::JoinHandle<T>,
) -> Result<T, RoleRunProcessError> {
    match tokio::time::timeout(ROLE_RUN_BLAME_CANCEL_GRACE, &mut task).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) => Err(RoleRunProcessError::Io),
        Err(_) => {
            task.abort();
            let _ = task.await;
            Err(RoleRunProcessError::Io)
        }
    }
}

async fn abort_role_run_io_tasks(
    stdin: tokio::task::JoinHandle<std::io::Result<()>>,
    stdout: tokio::task::JoinHandle<std::io::Result<BoundedRoleRunPipe>>,
    stderr: tokio::task::JoinHandle<std::io::Result<BoundedRoleRunPipe>>,
) {
    stdin.abort();
    stdout.abort();
    stderr.abort();
    let _ = stdin.await;
    let _ = stdout.await;
    let _ = stderr.await;
}

async fn terminate_and_reap_role_run_child(
    child: &mut tokio::process::Child,
    process_group: Option<i32>,
) {
    terminate_role_run_process_group(process_group, false);
    match tokio::time::timeout(ROLE_RUN_BLAME_CANCEL_GRACE, child.wait()).await {
        Ok(Ok(_)) => terminate_role_run_process_group(process_group, true),
        Ok(Err(_)) | Err(_) => {
            terminate_role_run_process_group(process_group, true);
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
    }
}

struct RoleRunProcessGroupGuard {
    process_group: Option<i32>,
    child_pid: Option<i32>,
}

impl RoleRunProcessGroupGuard {
    fn disarm(&mut self) {
        self.process_group = None;
        self.child_pid = None;
    }
}

impl Drop for RoleRunProcessGroupGuard {
    fn drop(&mut self) {
        terminate_role_run_process_group(self.process_group, true);
        reap_role_run_child(self.child_pid);
    }
}

#[cfg(unix)]
fn terminate_role_run_process_group(process_group: Option<i32>, hard: bool) {
    if let Some(process_group) = process_group {
        let signal = if hard {
            nix::sys::signal::Signal::SIGKILL
        } else {
            nix::sys::signal::Signal::SIGTERM
        };
        let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(process_group), signal);
    }
}

#[cfg(not(unix))]
fn terminate_role_run_process_group(_process_group: Option<i32>, _hard: bool) {}

#[cfg(unix)]
fn reap_role_run_child(child_pid: Option<i32>) {
    let Some(child_pid) = child_pid else {
        return;
    };
    let mut status = 0;
    loop {
        let result = unsafe { libc::waitpid(child_pid, &mut status, 0) };
        if result >= 0 {
            return;
        }
        let interrupted = std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted;
        if !interrupted {
            return;
        }
    }
}

#[cfg(not(unix))]
fn reap_role_run_child(_child_pid: Option<i32>) {}

fn validate_output_schema(
    workspace: &Path,
    output_schema: &Path,
    expected_workspace: RoleRunWorkspaceIdentity,
) -> Result<crate::headless_contract::StructuredOutputContract, RoleRunError> {
    if !output_schema.is_absolute() || output_schema == workspace {
        return Err(RoleRunError::InvalidOutputSchema);
    }
    let relative = output_schema
        .strip_prefix(workspace)
        .map_err(|_| RoleRunError::InvalidOutputSchema)?;
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty()
        || components.len() > MAX_ROLE_RUN_TREE_DEPTH
        || relative.as_os_str().as_encoded_bytes().len() > MAX_ROLE_RUN_TOOL_PATH_BYTES
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(RoleRunError::InvalidOutputSchema);
    }
    let names = components
        .iter()
        .map(|component| match component {
            Component::Normal(name) => name.to_str().ok_or(RoleRunError::InvalidOutputSchema),
            _ => Err(RoleRunError::InvalidOutputSchema),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let orchestrator_schema =
        names.len() >= 3 && names[0] == ".orchestrator" && names[1] == "schemas";
    if names.iter().enumerate().any(|(index, name)| {
        name.eq_ignore_ascii_case(".git")
            || (name.eq_ignore_ascii_case(".orchestrator") && !(orchestrator_schema && index == 0))
    }) {
        return Err(RoleRunError::InvalidOutputSchema);
    }
    let canonical = output_schema
        .canonicalize()
        .map_err(|_| RoleRunError::InvalidOutputSchema)?;
    if canonical != output_schema || !canonical.starts_with(workspace) || canonical == workspace {
        return Err(RoleRunError::InvalidOutputSchema);
    }
    #[cfg(unix)]
    {
        let relative = output_schema
            .strip_prefix(workspace)
            .map_err(|_| RoleRunError::InvalidOutputSchema)?;
        let RoleRunBoundNode::File(file) = open_role_run_bound_node_with_policy(
            workspace,
            expected_workspace,
            relative,
            if orchestrator_schema {
                RoleRunPrivatePathPolicy::OrchestratorSchema
            } else {
                RoleRunPrivatePathPolicy::Deny
            },
        )
        .map_err(|_| RoleRunError::InvalidOutputSchema)?
        else {
            return Err(RoleRunError::InvalidOutputSchema);
        };
        let bytes = read_role_run_bound_file(file, 256 * 1024)
            .map_err(|_| RoleRunError::InvalidOutputSchema)?;
        crate::headless_contract::StructuredOutputContract::from_bytes(&bytes)
            .map_err(|_| RoleRunError::InvalidOutputSchema)
    }
    #[cfg(not(unix))]
    {
        let _ = canonical;
        Err(RoleRunError::InvalidOutputSchema)
    }
}

fn validate_preflight_routes(routes: &[RoleRunPreflightRoute]) -> Result<(), RoleRunError> {
    if routes.len() > MAX_PREFLIGHT_ROUTES {
        return Err(RoleRunError::InvalidPreflight);
    }
    let mut providers = HashSet::with_capacity(routes.len());
    let mut total_models = 0usize;
    for route in routes {
        total_models = total_models
            .checked_add(route.models.len())
            .filter(|total| *total <= MAX_PREFLIGHT_MODELS_TOTAL)
            .ok_or(RoleRunError::InvalidPreflight)?;
        if !valid_provider(&route.provider)
            || route.models.is_empty()
            || route.models.len() > MAX_PREFLIGHT_MODELS_PER_ROUTE
            || !providers.insert(route.provider.as_str())
        {
            return Err(RoleRunError::InvalidPreflight);
        }
        let mut models = HashSet::with_capacity(route.models.len());
        if route
            .models
            .iter()
            .any(|model| !valid_model(model) || !models.insert(model.as_str()))
        {
            return Err(RoleRunError::InvalidPreflight);
        }
    }
    Ok(())
}

fn is_glob_meta(character: char) -> bool {
    matches!(character, '*' | '?' | '[' | ']' | '{' | '}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::io::Cursor;
    use std::path::PathBuf;

    struct Fixture {
        _temp: tempfile::TempDir,
        workspace: PathBuf,
        schema: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let workspace = temp.path().join("workspace");
            std::fs::create_dir_all(workspace.join(".orchestrator/schemas")).unwrap();
            std::fs::create_dir_all(workspace.join("src")).unwrap();
            let workspace = workspace.canonicalize().unwrap();
            let schema = workspace.join(".orchestrator/schemas/worker.schema.json");
            std::fs::write(
                &schema,
                br#"{"type":"object","additionalProperties":false,"required":["summary"],"properties":{"summary":{"type":"string"}}}"#,
            )
            .unwrap();
            Self {
                _temp: temp,
                workspace,
                schema,
            }
        }

        fn request(&self) -> Value {
            json!({
                "schema": ROLE_RUN_SCHEMA_V1,
                "request_id": "worker-INT-148-001",
                "provider": "openai",
                "model": "gpt-5.6-sol",
                "reasoning_effort": "high",
                "workspace": self.workspace,
                "prompt": "Implement the bounded protocol.",
                "output_schema": self.schema,
                "tools": [
                    "read_file", "list_dir", "search_files", "glob", "read_todos", "blame",
                    "apply_patch", "write_file"
                ],
                "grants": [
                    {"kind":"tool", "tool":"read_file"},
                    {"kind":"tool", "tool":"list_dir"},
                    {"kind":"tool", "tool":"search_files"},
                    {"kind":"tool", "tool":"glob"},
                    {"kind":"tool", "tool":"read_todos"},
                    {"kind":"tool", "tool":"blame"},
                    {"kind":"path", "tool":"apply_patch", "path_pattern": format!("{}/src/**", self.workspace.display())},
                    {"kind":"path", "tool":"write_file", "path_pattern": format!("{}/src/**", self.workspace.display())}
                ]
            })
        }

        fn validate(&self, value: &Value) -> Result<ValidatedRoleRunRequest, RoleRunError> {
            read_and_validate_request(
                Cursor::new(serde_json::to_vec(value).unwrap()),
                &self.workspace,
            )
        }
    }

    fn assert_code(error: RoleRunError, expected: &'static str) {
        assert_eq!(error.issue().code, expected);
    }

    fn active_role_run_cancellation() -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false))
    }

    #[test]
    fn valid_request_is_bounded_typed_and_debug_redacted() {
        let fixture = Fixture::new();
        let validated = fixture.validate(&fixture.request()).unwrap();

        assert_eq!(validated.request_id(), "worker-INT-148-001");
        assert_eq!(validated.provider(), "openai");
        assert_eq!(validated.model(), "gpt-5.6-sol");
        assert_eq!(
            validated.reasoning_effort(),
            crate::config::ReasoningEffort::High
        );
        assert_eq!(validated.workspace(), fixture.workspace);
        assert_eq!(validated.request.output_schema, fixture.schema);
        assert_eq!(validated.permission_grants().len(), 8);

        let shown = format!("{:?}", validated);
        assert!(!shown.contains("Implement the bounded protocol"));
        assert!(shown.contains("PROMPT REDACTED"));
    }

    #[test]
    fn request_rejects_unknown_fields_and_wrong_schema() {
        let fixture = Fixture::new();
        let mut request = fixture.request();
        request["allow_network"] = Value::Bool(false);
        assert_code(
            fixture.validate(&request).unwrap_err(),
            "invalid_request_envelope",
        );

        let mut request = fixture.request();
        request["schema"] = Value::String("ca.role_run.v2".into());
        assert_code(
            fixture.validate(&request).unwrap_err(),
            "invalid_request_envelope",
        );

        let mut request = fixture.request();
        request["grants"][0]["extra"] = Value::Bool(true);
        assert_code(
            fixture.validate(&request).unwrap_err(),
            "invalid_request_envelope",
        );

        let mut request = fixture.request();
        request["grants"][0] = json!({"kind":"command_prefix", "prefix":["git"]});
        assert_code(
            fixture.validate(&request).unwrap_err(),
            "invalid_request_envelope",
        );
    }

    #[test]
    fn reader_stops_after_the_one_mebibyte_limit() {
        let bytes = vec![b' '; MAX_ROLE_RUN_INPUT_BYTES + 1];
        assert_code(
            parse_request(Cursor::new(bytes)).unwrap_err(),
            "input_too_large",
        );
    }

    #[test]
    fn request_enforces_identifier_prompt_tool_and_grant_bounds() {
        let fixture = Fixture::new();

        let mut request = fixture.request();
        request["request_id"] = Value::String("x".repeat(MAX_REQUEST_ID_BYTES + 1));
        assert_code(
            fixture.validate(&request).unwrap_err(),
            "invalid_request_id",
        );

        let mut request = fixture.request();
        request["request_id"] = Value::String("unsafe request".into());
        assert_code(
            fixture.validate(&request).unwrap_err(),
            "invalid_request_id",
        );

        let mut request = fixture.request();
        request["prompt"] = Value::String("p".repeat(MAX_PROMPT_BYTES + 1));
        assert_code(fixture.validate(&request).unwrap_err(), "invalid_prompt");

        let mut request = fixture.request();
        request["tools"] = Value::Array(
            (0..=MAX_TOOLS)
                .map(|_| Value::String("read_file".into()))
                .collect(),
        );
        assert_code(fixture.validate(&request).unwrap_err(), "invalid_tools");

        let mut request = fixture.request();
        request["grants"] = Value::Array(
            (0..=MAX_GRANTS)
                .map(|index| {
                    json!({
                        "kind":"path",
                        "tool":"write_file",
                        "path_pattern": format!("{}/src/file-{index}", fixture.workspace.display())
                    })
                })
                .collect(),
        );
        assert_code(fixture.validate(&request).unwrap_err(), "invalid_grants");
    }

    #[test]
    fn authority_is_fixed_scoped_and_complete() {
        let fixture = Fixture::new();

        let mut request = fixture.request();
        request["tools"][0] = Value::String("run_command".into());
        assert_code(fixture.validate(&request).unwrap_err(), "invalid_tools");

        let mut request = fixture.request();
        request["grants"][0] = json!({"kind":"tool", "tool":"write_file"});
        assert_code(fixture.validate(&request).unwrap_err(), "invalid_grants");

        let mut request = fixture.request();
        request["grants"][0] = json!({
            "kind":"path",
            "tool":"read_file",
            "path_pattern": format!("{}/src/**", fixture.workspace.display())
        });
        assert_code(fixture.validate(&request).unwrap_err(), "invalid_grants");

        let mut request = fixture.request();
        request["grants"][6]["path_pattern"] = Value::String("/tmp/outside/**".into());
        assert_code(fixture.validate(&request).unwrap_err(), "invalid_grants");

        for protected in [".Git", ".ORCHESTRATOR", "OrAcLeS", "REFERENCE"] {
            let mut request = fixture.request();
            request["grants"][6]["path_pattern"] =
                Value::String(format!("{}/{protected}/**", fixture.workspace.display()));
            assert_code(fixture.validate(&request).unwrap_err(), "invalid_grants");
        }

        let mut request = fixture.request();
        request["grants"].as_array_mut().unwrap().pop();
        assert_code(fixture.validate(&request).unwrap_err(), "invalid_grants");

        let mut request = fixture.request();
        let duplicate = request["grants"][0].clone();
        request["grants"].as_array_mut().unwrap().push(duplicate);
        assert_code(fixture.validate(&request).unwrap_err(), "invalid_grants");
    }

    #[test]
    fn workspace_must_be_canonical_directory_equal_to_current_directory() {
        let fixture = Fixture::new();
        let other = tempfile::tempdir().unwrap().path().canonicalize().unwrap();
        assert_code(
            read_and_validate_request(
                Cursor::new(serde_json::to_vec(&fixture.request()).unwrap()),
                &other,
            )
            .unwrap_err(),
            "invalid_workspace",
        );

        let mut request = fixture.request();
        request["workspace"] = Value::String(format!("{}/.", fixture.workspace.display()));
        assert_code(fixture.validate(&request).unwrap_err(), "invalid_workspace");
    }

    #[test]
    fn schema_must_be_canonical_contained_regular_and_structurally_valid() {
        let fixture = Fixture::new();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(outside.path(), br#"{"type":"object"}"#).unwrap();

        let mut request = fixture.request();
        request["output_schema"] =
            Value::String(outside.path().canonicalize().unwrap().display().to_string());
        assert_code(
            fixture.validate(&request).unwrap_err(),
            "invalid_output_schema",
        );

        std::fs::write(&fixture.schema, br#"{"type":"string"}"#).unwrap();
        assert_code(
            fixture.validate(&fixture.request()).unwrap_err(),
            "invalid_output_schema",
        );
    }

    #[cfg(unix)]
    #[test]
    fn schema_and_grant_symlink_components_are_rejected() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let real = fixture.workspace.join("real-schemas");
        std::fs::create_dir(&real).unwrap();
        let real_schema = real.join("worker.json");
        std::fs::write(&real_schema, br#"{"type":"object"}"#).unwrap();
        let linked = fixture.workspace.join("linked-schemas");
        symlink(&real, &linked).unwrap();

        let mut request = fixture.request();
        request["output_schema"] = Value::String(linked.join("worker.json").display().to_string());
        assert_code(
            fixture.validate(&request).unwrap_err(),
            "invalid_output_schema",
        );

        let linked_src = fixture.workspace.join("linked-src");
        symlink(fixture.workspace.join("src"), &linked_src).unwrap();
        let mut request = fixture.request();
        request["grants"][6]["path_pattern"] =
            Value::String(format!("{}/linked-src/**", fixture.workspace.display()));
        assert_code(fixture.validate(&request).unwrap_err(), "invalid_grants");

        symlink(&real, fixture.workspace.join("src/nested-link")).unwrap();
        assert_code(
            fixture.validate(&fixture.request()).unwrap_err(),
            "invalid_grants",
        );
    }

    #[cfg(unix)]
    #[test]
    fn schema_hardlinks_are_rejected_before_provider_setup() {
        let fixture = Fixture::new();
        let outside = fixture._temp.path().join("outside-schema.json");
        std::fs::write(&outside, br#"{"type":"object"}"#).unwrap();
        std::fs::remove_file(&fixture.schema).unwrap();
        std::fs::hard_link(&outside, &fixture.schema).unwrap();

        assert_code(
            fixture.validate(&fixture.request()).unwrap_err(),
            "invalid_output_schema",
        );
    }

    #[cfg(unix)]
    #[test]
    fn schema_cannot_be_loaded_from_private_repository_state() {
        let fixture = Fixture::new();
        std::fs::create_dir(fixture.workspace.join(".git")).unwrap();
        let private_schema = fixture.workspace.join(".git/provider-visible.json");
        std::fs::write(&private_schema, br#"{"type":"object"}"#).unwrap();
        let mut request = fixture.request();
        request["output_schema"] = Value::String(private_schema.display().to_string());

        assert_code(
            fixture.validate(&request).unwrap_err(),
            "invalid_output_schema",
        );
    }

    #[cfg(unix)]
    #[test]
    fn schema_loader_rejects_a_replaced_workspace_root() {
        let fixture = Fixture::new();
        let identity = capture_role_run_workspace_identity(&fixture.workspace).unwrap();
        let original = fixture._temp.path().join("original-workspace");
        std::fs::rename(&fixture.workspace, &original).unwrap();
        std::fs::create_dir_all(fixture.workspace.join(".orchestrator/schemas")).unwrap();
        std::fs::write(
            &fixture.schema,
            br#"{"type":"object","properties":{"replacement":{"type":"string"}}}"#,
        )
        .unwrap();

        assert!(validate_output_schema(&fixture.workspace, &fixture.schema, identity).is_err());
    }

    #[test]
    fn role_run_tool_invocations_are_exact_contained_and_bounded() {
        let fixture = Fixture::new();
        let source = fixture.workspace.join("src/lib.rs");
        std::fs::write(&source, "fn bounded() {}\n").unwrap();

        let normalized = validate_role_run_tool_invocation(
            &fixture.workspace,
            "read_file",
            &json!({"path":"src/lib.rs","start_line":1,"end_line":1}),
        )
        .unwrap();
        assert_eq!(normalized["path"], source.to_string_lossy().as_ref());

        for (tool, arguments) in [
            (
                "write_file",
                json!({
                    "path": fixture.workspace.join("blocked.txt"),
                    "content": "blocked",
                    "cwd": fixture.workspace.join("src/lib.rs")
                }),
            ),
            (
                "apply_patch",
                json!({
                    "path": fixture.workspace.join("blocked.txt"),
                    "patch": "@@ -0,0 +1 @@\n+blocked",
                    "dest": fixture.workspace.join("src/lib.rs")
                }),
            ),
        ] {
            assert!(matches!(
                validate_role_run_tool_invocation(&fixture.workspace, tool, &arguments),
                Err(RoleRunInvocationError::InvalidArguments)
            ));
        }

        for arguments in [
            json!({"path":"../outside"}),
            json!({"path":"src/../lib.rs"}),
            json!({"path":"~/secret"}),
            json!({"path":".GiT/config"}),
            json!({"path":".ORCHESTRATOR/schemas/worker.schema.json"}),
            json!({"path":"src/lib.rs","unknown":"SECRET CANARY"}),
        ] {
            assert!(
                validate_role_run_tool_invocation(&fixture.workspace, "read_file", &arguments,)
                    .is_err()
            );
        }
        let outside = tempfile::NamedTempFile::new().unwrap();
        assert!(validate_role_run_tool_invocation(
            &fixture.workspace,
            "read_file",
            &json!({"path":outside.path()}),
        )
        .is_err());
        validate_role_run_tool_invocation(&fixture.workspace, "read_file", &json!({"path":"src"}))
            .unwrap();
        validate_role_run_tool_invocation(
            &fixture.workspace,
            "list_dir",
            &json!({"path":"src/lib.rs"}),
        )
        .unwrap();

        let oversized = fixture.workspace.join("src/oversized.rs");
        let file = std::fs::File::create(&oversized).unwrap();
        file.set_len(MAX_ROLE_RUN_FILE_BYTES + 1).unwrap();
        validate_role_run_tool_invocation(
            &fixture.workspace,
            "read_file",
            &json!({"path":"src/oversized.rs"}),
        )
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn role_run_rejects_case_aliases_on_case_insensitive_filesystems() {
        use std::os::unix::fs::MetadataExt;

        let fixture = Fixture::new();
        let exact = fixture.workspace.join("CaseSensitive.txt");
        let alias = fixture.workspace.join("casesensitive.txt");
        std::fs::write(&exact, "stable").unwrap();
        let Ok(alias_metadata) = std::fs::metadata(&alias) else {
            return;
        };
        let exact_metadata = std::fs::metadata(&exact).unwrap();
        if (alias_metadata.dev(), alias_metadata.ino())
            != (exact_metadata.dev(), exact_metadata.ino())
        {
            return;
        }

        validate_role_run_tool_invocation(
            &fixture.workspace,
            "write_file",
            &json!({"path":exact,"content":"allowed"}),
        )
        .unwrap();
        for (tool, arguments) in [
            ("read_file", json!({"path":alias})),
            ("write_file", json!({"path":alias,"content":"blocked"})),
        ] {
            assert!(matches!(
                validate_role_run_tool_invocation(&fixture.workspace, tool, &arguments),
                Err(RoleRunInvocationError::InvalidPath)
            ));
        }
    }

    #[test]
    fn role_run_positive_path_grants_are_separator_and_private_tree_safe() {
        let fixture = Fixture::new();
        let direct = fixture.workspace.join("src/direct.rs");
        let nested = fixture.workspace.join("src/deep/nested.rs");
        let hidden = fixture.workspace.join("src/.git/config");
        let grants = vec![crate::permission::PermissionMatcher::Path {
            tool: "write_file".into(),
            path_pattern: format!("{}/src/*.rs", fixture.workspace.display()),
        }];

        assert!(role_run_process_grants_authorize(
            &fixture.workspace,
            &grants,
            "write_file",
            &json!({"path":direct,"content":"ok"}),
        ));
        assert!(!role_run_process_grants_authorize(
            &fixture.workspace,
            &grants,
            "write_file",
            &json!({"path":nested,"content":"blocked"}),
        ));

        let recursive = vec![crate::permission::PermissionMatcher::Path {
            tool: "write_file".into(),
            path_pattern: format!("{}/src/**", fixture.workspace.display()),
        }];
        assert!(!role_run_process_grants_authorize(
            &fixture.workspace,
            &recursive,
            "write_file",
            &json!({"path":hidden,"content":"blocked"}),
        ));
    }

    #[test]
    fn role_run_apply_patch_is_bounded_linear_and_cancellable() {
        let active = std::sync::atomic::AtomicBool::new(false);
        let patched = apply_role_run_unified_patch(
            "a\nb\nc\nd",
            "@@ -1,2 +1,2 @@\n a\n-b\n+B\n@@ -4,1 +4,2 @@\n d\n+D2",
            &active,
        )
        .unwrap();
        assert_eq!(patched, "a\nB\nc\nd\nD2");

        assert!(
            apply_role_run_unified_patch("a\nb", "@@ -1,1 +1,1 @@\n-wrong\n+changed", &active,)
                .is_err()
        );
        let cancelled = std::sync::atomic::AtomicBool::new(true);
        assert!(
            apply_role_run_unified_patch("a\nb", "@@ -1,1 +1,1 @@\n-a\n+changed", &cancelled,)
                .is_err()
        );

        let excessive = "@@ -0,0 +1,1 @@\n+x\n".repeat(MAX_ROLE_RUN_PATCH_HUNKS + 1);
        assert!(apply_role_run_unified_patch("", &excessive, &active).is_err());
    }

    #[test]
    fn role_run_apply_patch_validates_both_hunk_coordinate_systems() {
        let active = std::sync::atomic::AtomicBool::new(false);
        for (original, patch, expected) in [
            ("a\nb", "@@ -1,1 +1,1 @@\n-a\n+A", "A\nb"),
            ("a\nb", "@@ -0,0 +1,1 @@\n+x", "x\na\nb"),
            ("a\nb", "@@ -1,0 +2,1 @@\n+x", "a\nx\nb"),
            ("a\nb", "@@ -1,1 +0,0 @@\n-a", "b"),
            ("a\nb", "@@ -2,1 +1,0 @@\n-b", "a"),
            (
                "a\nb\nc\nd",
                "@@ -0,0 +1,1 @@\n+x\n@@ -4,1 +5,1 @@\n-d\n+D",
                "x\na\nb\nc\nD",
            ),
            (
                "a\nb\nc\nd",
                "@@ -1,1 +0,0 @@\n-a\n@@ -4,1 +3,1 @@\n-d\n+D",
                "b\nc\nD",
            ),
        ] {
            assert_eq!(
                apply_role_run_unified_patch(original, patch, &active).unwrap(),
                expected,
                "valid patch coordinates were rejected: {patch}"
            );
        }

        for (original, patch) in [
            ("a\nb", "@@ -1,1 +2,1 @@\n-a\n+A"),
            ("a\nb", "@@ -0,0 +2,1 @@\n+x"),
            ("a\nb", "@@ -1,0 +1,1 @@\n+x"),
            ("a\nb", "@@ -1,1 +1,0 @@\n-a"),
            ("a\nb", "@@ -2,1 +0,0 @@\n-b"),
            ("a\nb\nc\nd", "@@ -0,0 +1,1 @@\n+x\n@@ -4,1 +4,1 @@\n-d\n+D"),
            ("a\nb\nc\nd", "@@ -1,1 +0,0 @@\n-a\n@@ -4,1 +4,1 @@\n-d\n+D"),
        ] {
            assert!(
                apply_role_run_unified_patch(original, patch, &active).is_err(),
                "inconsistent new-file coordinates were accepted: {patch}"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn role_run_mutation_boundary_rejects_hardlinks_and_validation_to_open_symlink_swaps() {
        use std::os::unix::fs::{symlink, MetadataExt};

        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let workspace = workspace.canonicalize().unwrap();

        let ordinary = workspace.join("ordinary.txt");
        std::fs::write(&ordinary, "before").unwrap();
        let ordinary_inode = std::fs::metadata(&ordinary).unwrap().ino();
        let ordinary_result = execute_role_run_mutation(
            &workspace,
            capture_role_run_workspace_identity(&workspace).unwrap(),
            active_role_run_cancellation(),
            crate::headless::HeadlessOutputRedactions::default(),
            "write_file",
            &json!({"path":ordinary,"content":"after"}),
        )
        .await;
        assert!(ordinary_result.is_success());
        assert_eq!(std::fs::read_to_string(&ordinary).unwrap(), "after");
        assert_ne!(std::fs::metadata(&ordinary).unwrap().ino(), ordinary_inode);

        let secret = "credential-prefix\ncredential-suffix";
        let composed = workspace.join("composed-secret.txt");
        let redactions = crate::headless::HeadlessOutputRedactions::exact([secret.to_string()]);
        let prefix_result = execute_role_run_mutation(
            &workspace,
            capture_role_run_workspace_identity(&workspace).unwrap(),
            active_role_run_cancellation(),
            redactions.clone(),
            "write_file",
            &json!({"path":composed,"content":"credential-prefix"}),
        )
        .await;
        assert!(prefix_result.is_success());
        let composed_result = execute_role_run_mutation(
            &workspace,
            capture_role_run_workspace_identity(&workspace).unwrap(),
            active_role_run_cancellation(),
            redactions,
            "apply_patch",
            &json!({
                "path":composed,
                "patch":"@@ -1,1 +1,2 @@\n credential-prefix\n+credential-suffix"
            }),
        )
        .await;
        assert!(matches!(
            composed_result,
            crate::agent::registry::ToolExecutionResult::Failure(_)
        ));
        assert_eq!(
            std::fs::read_to_string(&composed).unwrap(),
            "credential-prefix"
        );

        let outside_hardlink = temp.path().join("outside-hardlink.txt");
        let hardlink = workspace.join("hardlink.txt");
        std::fs::write(&outside_hardlink, "outside-hardlink-canary").unwrap();
        std::fs::hard_link(&outside_hardlink, &hardlink).unwrap();
        let hardlink_result = execute_role_run_mutation(
            &workspace,
            capture_role_run_workspace_identity(&workspace).unwrap(),
            active_role_run_cancellation(),
            crate::headless::HeadlessOutputRedactions::default(),
            "write_file",
            &json!({"path":hardlink,"content":"blocked"}),
        )
        .await;
        assert!(matches!(
            hardlink_result,
            crate::agent::registry::ToolExecutionResult::Failure(_)
        ));
        assert_eq!(
            std::fs::read_to_string(&outside_hardlink).unwrap(),
            "outside-hardlink-canary"
        );
        assert_eq!(
            std::fs::read_to_string(&hardlink).unwrap(),
            "outside-hardlink-canary"
        );

        let outside_symlink = temp.path().join("outside-symlink.txt");
        let swapped = workspace.join("swapped.txt");
        std::fs::write(&outside_symlink, "outside-symlink-canary").unwrap();
        std::fs::write(&swapped, "initially-valid").unwrap();
        let arguments = json!({"path":swapped,"content":"blocked"});
        validate_role_run_tool_invocation(&workspace, "write_file", &arguments).unwrap();
        std::fs::remove_file(&swapped).unwrap();
        symlink(&outside_symlink, &swapped).unwrap();

        let swapped_result = execute_role_run_mutation(
            &workspace,
            capture_role_run_workspace_identity(&workspace).unwrap(),
            active_role_run_cancellation(),
            crate::headless::HeadlessOutputRedactions::default(),
            "write_file",
            &arguments,
        )
        .await;

        assert!(matches!(
            swapped_result,
            crate::agent::registry::ToolExecutionResult::Failure(_)
        ));
        assert_eq!(
            std::fs::read_to_string(&outside_symlink).unwrap(),
            "outside-symlink-canary"
        );
        assert!(std::fs::symlink_metadata(&swapped)
            .unwrap()
            .file_type()
            .is_symlink());

        let cancelled_target = workspace.join("cancelled.txt");
        std::fs::write(&cancelled_target, "before-cancel").unwrap();
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let cancelled_result = execute_role_run_mutation(
            &workspace,
            capture_role_run_workspace_identity(&workspace).unwrap(),
            cancelled,
            crate::headless::HeadlessOutputRedactions::default(),
            "write_file",
            &json!({"path":cancelled_target,"content":"after-cancel"}),
        )
        .await;
        assert!(matches!(
            cancelled_result,
            crate::agent::registry::ToolExecutionResult::Failure(_)
        ));
        assert_eq!(
            std::fs::read_to_string(&cancelled_target).unwrap(),
            "before-cancel"
        );

        let root_identity = capture_role_run_workspace_identity(&workspace).unwrap();
        let root_target = workspace.join("root-swap.txt");
        std::fs::write(&root_target, "original-root").unwrap();
        let original_workspace = temp.path().join("original-workspace");
        std::fs::rename(&workspace, &original_workspace).unwrap();
        std::fs::create_dir(&workspace).unwrap();
        std::fs::write(workspace.join("root-swap.txt"), "replacement-root").unwrap();
        let root_swap_result = execute_role_run_mutation(
            &workspace,
            root_identity,
            active_role_run_cancellation(),
            crate::headless::HeadlessOutputRedactions::default(),
            "write_file",
            &json!({"path":workspace.join("root-swap.txt"),"content":"blocked"}),
        )
        .await;
        assert!(matches!(
            root_swap_result,
            crate::agent::registry::ToolExecutionResult::Failure(_)
        ));
        assert_eq!(
            std::fs::read_to_string(workspace.join("root-swap.txt")).unwrap(),
            "replacement-root"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn role_run_read_boundary_rejects_hardlinks_root_replacement_and_symlink_swaps() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let identity = capture_role_run_workspace_identity(&workspace).unwrap();
        let outside_file = temp.path().join("outside.txt");
        let outside_directory = temp.path().join("outside-directory");
        std::fs::write(&outside_file, "OUTSIDE-READ-CANARY TODO").unwrap();
        std::fs::create_dir(&outside_directory).unwrap();
        std::fs::write(
            outside_directory.join("private.txt"),
            "OUTSIDE-DIRECTORY-CANARY TODO",
        )
        .unwrap();

        for (tool, arguments) in [
            {
                let target = workspace.join("hardlink-read.txt");
                std::fs::hard_link(&outside_file, &target).unwrap();
                ("read_file", json!({"path":target}))
            },
            {
                let target = workspace.join("hardlink-list");
                std::fs::create_dir(&target).unwrap();
                std::fs::hard_link(&outside_file, target.join("private.txt")).unwrap();
                ("list_dir", json!({"path":target}))
            },
            {
                let target = workspace.join("hardlink-search");
                std::fs::create_dir(&target).unwrap();
                std::fs::hard_link(&outside_file, target.join("private.txt")).unwrap();
                ("search_files", json!({"path":target,"pattern":"CANARY"}))
            },
            {
                let target = workspace.join("hardlink-glob");
                std::fs::create_dir(&target).unwrap();
                std::fs::hard_link(&outside_file, target.join("private.txt")).unwrap();
                ("glob", json!({"path":target,"pattern":"**/*"}))
            },
            {
                let target = workspace.join("hardlink-todos");
                std::fs::create_dir(&target).unwrap();
                std::fs::hard_link(&outside_file, target.join("private.txt")).unwrap();
                ("read_todos", json!({"path":target}))
            },
        ] {
            let result = execute_role_run_read(
                &workspace,
                identity,
                active_role_run_cancellation(),
                tool,
                &arguments,
            )
            .await;
            assert!(
                matches!(
                    &result,
                    crate::agent::registry::ToolExecutionResult::Failure(_)
                ),
                "{tool} accepted a hardlink"
            );
            assert!(!result.model_text().contains("OUTSIDE"));
        }

        for (tool, arguments, target) in [
            {
                let target = workspace.join("swapped-read.txt");
                std::fs::write(&target, "safe").unwrap();
                ("read_file", json!({"path":target}), target)
            },
            {
                let target = workspace.join("swapped-list");
                std::fs::create_dir(&target).unwrap();
                ("list_dir", json!({"path":target}), target)
            },
            {
                let target = workspace.join("swapped-search");
                std::fs::create_dir(&target).unwrap();
                std::fs::write(target.join("safe.txt"), "safe").unwrap();
                (
                    "search_files",
                    json!({"path":target,"pattern":"CANARY"}),
                    target,
                )
            },
            {
                let target = workspace.join("swapped-glob");
                std::fs::create_dir(&target).unwrap();
                std::fs::write(target.join("safe.txt"), "safe").unwrap();
                ("glob", json!({"path":target,"pattern":"**/*"}), target)
            },
            {
                let target = workspace.join("swapped-todos");
                std::fs::create_dir(&target).unwrap();
                std::fs::write(target.join("safe.txt"), "safe").unwrap();
                ("read_todos", json!({"path":target}), target)
            },
        ] {
            validate_role_run_tool_invocation(&workspace, tool, &arguments).unwrap();
            if target.is_dir() {
                std::fs::remove_dir_all(&target).unwrap();
                symlink(&outside_directory, &target).unwrap();
            } else {
                std::fs::remove_file(&target).unwrap();
                symlink(&outside_file, &target).unwrap();
            }
            let result = execute_role_run_read(
                &workspace,
                identity,
                active_role_run_cancellation(),
                tool,
                &arguments,
            )
            .await;
            assert!(
                matches!(
                    &result,
                    crate::agent::registry::ToolExecutionResult::Failure(_)
                ),
                "{tool} followed a swapped symlink"
            );
            assert!(!result.model_text().contains("OUTSIDE"));
        }

        let root_target = workspace.join("root-target.txt");
        std::fs::write(&root_target, "safe").unwrap();
        let arguments = json!({"path":root_target});
        validate_role_run_tool_invocation(&workspace, "read_file", &arguments).unwrap();
        let original_workspace = temp.path().join("original-workspace");
        std::fs::rename(&workspace, &original_workspace).unwrap();
        std::fs::create_dir(&workspace).unwrap();
        std::fs::write(workspace.join("root-target.txt"), "ROOT-SWAP-CANARY").unwrap();
        let result = execute_role_run_read(
            &workspace,
            identity,
            active_role_run_cancellation(),
            "read_file",
            &arguments,
        )
        .await;
        assert!(matches!(
            &result,
            crate::agent::registry::ToolExecutionResult::Failure(_)
        ));
        assert!(!result.model_text().contains("ROOT-SWAP-CANARY"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn role_run_glob_hides_private_runtime_trees() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.workspace.join(".git")).unwrap();
        std::fs::write(fixture.workspace.join(".git/config"), "PRIVATE-GIT-CANARY").unwrap();
        std::fs::write(fixture.workspace.join("src/lib.rs"), "pub fn visible() {}").unwrap();
        let result = execute_role_run_read(
            &fixture.workspace,
            capture_role_run_workspace_identity(&fixture.workspace).unwrap(),
            active_role_run_cancellation(),
            "glob",
            &json!({"path":fixture.workspace,"pattern":"**/*"}),
        )
        .await;
        assert!(result.is_success());
        assert!(result.model_text().contains("src/lib.rs"));
        assert!(!result.model_text().contains(".git"));
        assert!(!result.model_text().contains(".orchestrator"));
        assert!(!result.model_text().contains("PRIVATE-GIT-CANARY"));
    }

    #[test]
    fn role_run_glob_rejects_escape_and_validates_its_literal_subtree() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.workspace.join("src/nested")).unwrap();
        std::fs::write(
            fixture.workspace.join("src/nested/lib.rs"),
            "fn main() {}\n",
        )
        .unwrap();

        let normalized = validate_role_run_tool_invocation(
            &fixture.workspace,
            "glob",
            &json!({"path":".","pattern":"src/**/*.rs"}),
        )
        .unwrap();
        assert_eq!(
            normalized["path"],
            fixture.workspace.to_string_lossy().as_ref()
        );
        for pattern in [
            "../*",
            "/tmp/*",
            "src/../../*",
            "src\\**\\*.rs",
            ".[.]/outside/*",
            ".?/outside/*",
            ".git/**",
            ".[gG][iI][tT]/**",
            ".orchestrato?/**",
            "[",
        ] {
            assert!(validate_role_run_tool_invocation(
                &fixture.workspace,
                "glob",
                &json!({"path":".","pattern":pattern}),
            )
            .is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn role_run_read_validation_rejects_final_symlinks_and_defers_other_tree_checks() {
        use std::ffi::CString;
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let linked = fixture.workspace.join("src/linked.rs");
        symlink(outside.path(), &linked).unwrap();
        assert!(validate_role_run_tool_invocation(
            &fixture.workspace,
            "read_file",
            &json!({"path":"src/linked.rs"}),
        )
        .is_err());
        validate_role_run_tool_invocation(
            &fixture.workspace,
            "search_files",
            &json!({"path":"src","pattern":"canary"}),
        )
        .unwrap();
        std::fs::remove_file(&linked).unwrap();

        let fifo = fixture.workspace.join("src/blocked.fifo");
        let fifo_c = CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
        validate_role_run_tool_invocation(&fixture.workspace, "list_dir", &json!({"path":"src"}))
            .unwrap();
        std::fs::remove_file(&fifo).unwrap();

        let skipped = fixture.workspace.join("target/cache");
        std::fs::create_dir_all(&skipped).unwrap();
        symlink(outside.path(), skipped.join("ignored-link")).unwrap();
        validate_role_run_tool_invocation(
            &fixture.workspace,
            "search_files",
            &json!({"path":".","pattern":"bounded"}),
        )
        .unwrap();
    }

    #[test]
    fn only_nonreserved_named_profiles_are_accepted() {
        assert_eq!(
            validate_named_profile("codex-team").unwrap(),
            crate::runtime::ActiveProfile::Named("codex-team".into())
        );
        for rejected in ["", "default", "bare", "Upper", "with space"] {
            assert_code(
                validate_named_profile(rejected).unwrap_err(),
                "invalid_profile",
            );
        }
    }

    #[test]
    fn preflight_is_exact_bounded_and_uses_static_safe_issues() {
        let receipt = RoleRunPreflight::new(
            "codex-team",
            true,
            vec![RoleRunPreflightRoute {
                provider: "openai".into(),
                models: vec!["gpt-5.6-sol".into()],
            }],
            vec![],
        )
        .unwrap();
        let encoded = serde_json::to_value(&receipt).unwrap();
        assert_eq!(encoded["schema"], ROLE_RUN_PREFLIGHT_SCHEMA_V1);
        assert_eq!(encoded["ready"], true);
        assert_eq!(encoded["sandbox_required"], true);

        let mut unknown = encoded.clone();
        unknown["credential"] = Value::String("SECRET CANARY".into());
        assert!(serde_json::from_value::<RoleRunPreflight>(unknown).is_err());

        let mut wrong_schema = encoded.clone();
        wrong_schema["schema"] = Value::String("ca.role_run_preflight.v2".into());
        assert!(serde_json::from_value::<RoleRunPreflight>(wrong_schema).is_err());

        let mut unknown_route = encoded.clone();
        unknown_route["routes"][0]["credential"] = Value::String("SECRET CANARY".into());
        assert!(serde_json::from_value::<RoleRunPreflight>(unknown_route).is_err());

        let unhealthy = RoleRunPreflight::new(
            "codex-team",
            false,
            vec![],
            vec![RoleRunPreflightIssue::SandboxNotRequired],
        )
        .unwrap();
        let encoded = serde_json::to_string(&unhealthy).unwrap();
        assert!(!unhealthy.ready);
        assert!(!encoded.contains("SECRET CANARY"));
        assert!(encoded.contains("sandbox_not_required"));

        let mut output = Vec::new();
        assert_eq!(write_preflight_receipt(&unhealthy, &mut output), 0);
        let emitted: RoleRunPreflight = serde_json::from_slice(&output).unwrap();
        assert!(!emitted.ready);
    }

    #[tokio::test]
    async fn safe_role_run_blame_uses_only_the_exact_repository() {
        let Some(git) = trusted_role_run_git_binary().ok() else {
            return;
        };
        let fixture = Fixture::new();
        let source = fixture.workspace.join("src/lib.rs");
        std::fs::write(&source, "fn blame_canary() {}\n").unwrap();
        let run_git = |arguments: &[&str]| {
            std::process::Command::new(&git)
                .args(arguments)
                .current_dir(&fixture.workspace)
                .env_clear()
                .env("HOME", "/var/empty")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .status()
                .unwrap()
        };
        assert!(run_git(&["init", "--quiet"]).success());
        assert!(run_git(&["add", "--", "src/lib.rs"]).success());
        assert!(run_git(&[
            "-c",
            "user.name=Role Run Test",
            "-c",
            "user.email=role-run@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ])
        .success());

        let result = execute_role_run_blame(
            &fixture.workspace,
            capture_role_run_workspace_identity(&fixture.workspace).unwrap(),
            active_role_run_cancellation(),
            &json!({"path":source,"start_line":1,"end_line":1}),
            10,
        )
        .await;
        assert!(matches!(
            &result,
            crate::agent::registry::ToolExecutionResult::Success(output)
                if output.contains("blame_canary")
        ));

        let nested = fixture.workspace.join("src/nested");
        std::fs::create_dir(&nested).unwrap();
        let nested_file = nested.join("file.rs");
        std::fs::write(&nested_file, "outside exact root\n").unwrap();
        let result = execute_role_run_blame(
            &nested,
            capture_role_run_workspace_identity(&nested).unwrap(),
            active_role_run_cancellation(),
            &json!({"path":nested_file}),
            10,
        )
        .await;
        assert!(matches!(
            result,
            crate::agent::registry::ToolExecutionResult::Failure(_)
        ));

        let linked = fixture.workspace.join("linked-worktree");
        let linked_admin = fixture.workspace.join("linked-admin");
        std::fs::create_dir(&linked).unwrap();
        std::fs::create_dir(&linked_admin).unwrap();
        std::fs::write(
            linked.join(".git"),
            format!("gitdir: {}\n", linked_admin.display()),
        )
        .unwrap();
        std::fs::write(
            linked_admin.join("gitdir"),
            format!("{}\n", linked.join(".git").display()),
        )
        .unwrap();
        let linked_root = open_role_run_workspace_root(
            &linked,
            capture_role_run_workspace_identity(&linked).unwrap(),
        )
        .unwrap();
        assert!(role_run_workspace_git_repository(&linked_root, &linked).is_some());

        let crafted = fixture.workspace.join("crafted");
        let unrelated_admin = fixture.workspace.join("unrelated-admin");
        std::fs::create_dir_all(&crafted).unwrap();
        std::fs::create_dir_all(&unrelated_admin).unwrap();
        std::fs::write(
            crafted.join(".git"),
            format!("gitdir: {}\n", unrelated_admin.display()),
        )
        .unwrap();
        let root = open_role_run_workspace_root(
            &crafted,
            capture_role_run_workspace_identity(&crafted).unwrap(),
        )
        .unwrap();
        assert!(role_run_workspace_git_repository(&root, &crafted).is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dropping_role_run_process_reaps_its_descendant_group() {
        let temp = tempfile::tempdir().unwrap();
        let child_pid_file = temp.path().join("child.pid");
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(
                "trap '' TERM; /bin/sh -c 'trap \"\" TERM; exec /bin/sleep 30' & echo $! > \"$1\"; wait",
            )
            .arg("role-run-test")
            .arg(&child_pid_file);
        let task = tokio::spawn(run_bounded_role_run_process(
            command,
            Vec::new(),
            Duration::from_secs(30),
        ));
        let mut child_pid = None;
        for _ in 0..100 {
            if let Ok(raw) = std::fs::read_to_string(&child_pid_file) {
                child_pid = raw.trim().parse::<i32>().ok();
                if child_pid.is_some() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let child_pid = child_pid.expect("fixture descendant started");
        task.abort();
        let _ = task.await;

        let mut gone = false;
        for _ in 0..100 {
            if nix::sys::signal::kill(nix::unistd::Pid::from_raw(child_pid), None).is_err() {
                gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if !gone {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(child_pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        assert!(gone, "role-run descendant survived cancellation");
    }

    #[test]
    fn validation_errors_are_bounded_static_and_serializable() {
        let fixture = Fixture::new();
        let mut request = fixture.request();
        request["prompt"] = Value::String("SECRET CANARY".repeat(MAX_PROMPT_BYTES));
        let error = fixture.validate(&request).unwrap_err();
        let issue = error.issue();
        let encoded = serde_json::to_string(&issue).unwrap();
        assert!(encoded.len() < 256);
        assert!(!encoded.contains("SECRET CANARY"));
    }

    #[cfg(unix)]
    #[test]
    fn role_run_path_spelling_never_descends_through_intermediate_symlinks() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let outside = fixture._temp.path().join("outside-sentinel");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("DO-NOT-TRAVERSE"), "outside").unwrap();
        symlink(&outside, fixture.workspace.join("src/outside-link")).unwrap();

        for (tool, arguments) in [
            (
                "read_file",
                json!({"path":"src/outside-link/DO-NOT-TRAVERSE"}),
            ),
            (
                "write_file",
                json!({
                    "path":"src/outside-link/DO-NOT-TRAVERSE",
                    "content":"blocked"
                }),
            ),
        ] {
            assert!(matches!(
                validate_role_run_tool_invocation(&fixture.workspace, tool, &arguments),
                Err(RoleRunInvocationError::InvalidPath)
            ));
        }

        let gate = fixture.workspace.join("src/swap-gate");
        let held_gate = fixture._temp.path().join("held-swap-gate");
        std::fs::create_dir(&gate).unwrap();
        let workspace_root = open_role_run_workspace_root(
            &fixture.workspace,
            capture_role_run_workspace_identity(&fixture.workspace).unwrap(),
        )
        .unwrap();
        let RoleRunBoundNode::Directory(src_directory) =
            open_role_run_child(&workspace_root, "src").unwrap()
        else {
            panic!("fixture src must be a directory");
        };
        let cancellation = std::sync::atomic::AtomicBool::new(false);
        assert!(read_role_run_directory_names(
            &src_directory,
            MAX_ROLE_RUN_LIST_ENTRIES,
            &cancellation,
        )
        .unwrap()
        .iter()
        .any(|name| name == "swap-gate"));
        std::fs::rename(&gate, &held_gate).unwrap();
        symlink(&outside, &gate).unwrap();
        assert!(open_role_run_child(&src_directory, "swap-gate").is_err());

        assert_eq!(
            std::fs::read_to_string(outside.join("DO-NOT-TRAVERSE")).unwrap(),
            "outside"
        );

        validate_role_run_tool_invocation(
            &fixture.workspace,
            "write_file",
            &json!({"path":"src/missing-final.rs","content":"allowed"}),
        )
        .unwrap();

        let over_depth = std::iter::repeat_n("segment", MAX_ROLE_RUN_TREE_DEPTH + 1)
            .collect::<Vec<_>>()
            .join("/");
        assert!(matches!(
            validate_role_run_tool_invocation(
                &fixture.workspace,
                "read_file",
                &json!({"path":over_depth}),
            ),
            Err(RoleRunInvocationError::InvalidPath)
        ));
    }
}
