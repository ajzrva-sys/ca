//! CLI entry point.
//!
//! Owns Clap parsing and binary-level subcommand dispatch. This module should
//! stay thin: parse flags, choose the app entry point, and let `app` conduct the
//! orchestration behavior.
//!
//! Read when changing command-line flags or user-facing subcommands. Do not read
//! for queue parsing, verification, role prompts, guard policy, or state-machine
//! routing.

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "codex-team")]
#[command(version)]
#[command(about = "Supervised local project orchestrator with pluggable role engines", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
#[allow(clippy::enum_variant_names)]
enum Commands {
    /// Compatibility adapters for typed automation setup and execution.
    Automation {
        #[command(subcommand)]
        command: AutomationCommands,
    },
    /// Private repository-wide mechanistic coordinator.
    Broker {
        #[command(subcommand)]
        command: BrokerCommands,
    },
    /// Typed remote actions; requests contain no arbitrary shell commands.
    Remote {
        #[command(subcommand)]
        command: RemoteCommands,
    },
    /// Typed automation workflow compatibility entry point.
    Workflow {
        #[command(subcommand)]
        command: WorkflowCommands,
    },
    /// Execute typed repository queries or evidence without a provider.
    Mechanistic {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long, default_value = "-")]
        input: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Preview and publish an exact protected-content baseline.
    ProtectedBaseline {
        #[command(subcommand)]
        command: ProtectedBaselineCommands,
    },
    /// Create .orchestrator files and default project documents.
    Init {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// High-level mission for CEO to turn into ROADMAP.md.
        #[arg(long)]
        mission: String,

        /// Commit accepted work automatically or pause for an exact approval.
        #[arg(long, value_enum)]
        commit_mode: Option<CommitModeArg>,

        /// Refuse execution unless the repository starts clean.
        #[arg(long)]
        require_clean_start: bool,
    },

    /// Commit one exact, previously reviewed manual pending intent.
    CommitPending {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Stable pending intent ID from status or a cycle receipt.
        #[arg(long)]
        id: String,

        /// Exact SHA-256 digest from the pending intent.
        #[arg(long)]
        expected_digest: String,

        /// Exact repository revision observed during review.
        #[arg(long)]
        expected_revision: String,

        /// Emit one machine-readable receipt.
        #[arg(long)]
        json: bool,
    },

    /// Preview or digest-bound apply one strict external task object.
    TaskImport {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// JSON file to read, or - for stdin.
        #[arg(long)]
        input: PathBuf,

        /// Exact SHA-256 digest returned by preview.
        #[arg(long)]
        accept_digest: Option<String>,

        /// Exact repository revision returned by preview.
        #[arg(long)]
        expected_revision: Option<String>,

        /// Emit one machine-readable receipt.
        #[arg(long)]
        json: bool,
    },

    /// Review and manage controlled evidence-only learning recipes.
    Recipe {
        #[command(subcommand)]
        command: RecipeCommands,
    },

    /// Enroll and operate the machine-local autonomous learning supervisor.
    SelfImprove {
        #[command(subcommand)]
        command: SelfImproveCommands,
    },

    /// Run a Rust-owned repository hygiene check without a shell.
    Verify {
        /// Built-in verification check.
        #[arg(value_enum)]
        check: VerifyCheckArg,

        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Emit one machine-readable receipt.
        #[arg(long)]
        json: bool,
    },

    /// Run exactly one orchestration cycle.
    Step {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Emit a machine-readable cycle receipt JSON object after the step.
        #[arg(long)]
        json: bool,

        /// Require this exact repository revision before any cycle mutation.
        #[arg(long)]
        expected_revision: Option<String>,
    },

    /// Run cycles until the mission is complete, max cycles is reached, or approval/blocker stops progress.
    Run {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Override config max_cycles for this invocation.
        #[arg(long)]
        max_cycles: Option<usize>,

        /// Stop instead of automatically escalating back to CEO when Project Manager/Senior Dev asks for direction.
        #[arg(long)]
        pause_on_escalation: bool,

        /// Continue through all available work without prompting after each cycle.
        #[arg(long)]
        auto_continue: bool,
    },

    /// Print current orchestrator state.
    Status {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Emit machine-readable status JSON instead of human text.
        #[arg(long)]
        json: bool,

        /// Observe-only: load state without queue recover/save mutation (still locks).
        #[arg(long)]
        observe: bool,
    },

    /// Update the persisted mission without running AI roles.
    Mission {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// New high-level mission text.
        #[arg(long = "set")]
        mission: String,
    },

    /// Show or update role models, reasoning effort, and timeout.
    Models {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Role to update or inspect. Aliases: ceo, pm, senior-dev.
        #[arg(long, value_enum)]
        role: Option<RoleArg>,

        /// New model name for the role.
        #[arg(long)]
        model: Option<String>,

        /// New reasoning effort/speed for the role, such as low, medium, high, or xhigh.
        #[arg(long)]
        effort: Option<String>,

        /// New timeout in seconds for the role.
        #[arg(long)]
        timeout_seconds: Option<u64>,
    },

    /// Record a human disposition for an active pause without an interactive prompt.
    Disposition {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Human decision to record for the active pause.
        #[arg(long)]
        decision: DispositionDecisionArg,

        /// Refresh the dirty-adoption report without clearing the pause or running roles.
        #[arg(long)]
        refresh_report: bool,

        /// Emit a machine-readable disposition receipt JSON object.
        #[arg(long)]
        json: bool,

        /// Optional exact repository revision; must be paired with --pause-fingerprint.
        #[arg(long)]
        expected_revision: Option<String>,

        /// Optional exact active-pause fingerprint; must be paired with --expected-revision.
        #[arg(long)]
        pause_fingerprint: Option<String>,
    },

    /// Report model-call token estimates, Rust skips, and deterministic verification work.
    Cost {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,
    },

    /// Score synthetic and recent task profiles against advisory route lanes.
    RouteBench {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Print the benchmark report as JSON instead of Markdown.
        #[arg(long)]
        json: bool,
    },

    /// Validate Codex, config, Git tracking, prompts, and configured models.
    Doctor {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Emit a machine-readable doctor receipt JSON object.
        #[arg(long)]
        json: bool,
    },

    /// Export runnable command scripts for approved NEXT_STEPS.md tasks without using AI.
    Commands {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Optional task ID to export. Defaults to every currently approved parsed task.
        #[arg(long)]
        task: Option<String>,
    },

    /// Experimental deterministic watch/debounce scaffold for one task.
    Watch {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Task ID whose allowed/read-only files should seed the watch set.
        #[arg(long)]
        task: Option<String>,

        /// Apply safe Rust-owned efficiency mitigations in .orchestrator.
        #[arg(long)]
        self_heal: bool,
    },

    /// Run a bounded target-work + efficiency-watch + orchestrator-repair loop.
    WatchLoop {
        /// Target repository/project root to advance.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Orchestrator repository to receive efficiency feedback and repair itself.
        #[arg(long)]
        orchestrator_repo: Option<PathBuf>,

        /// Mission to use if --orchestrator-repo has not been initialized yet.
        #[arg(
            long,
            default_value = "Improve codex-team orchestration efficiency while preserving safe bounded automation"
        )]
        orchestrator_mission: String,

        /// Maximum target iterations.
        #[arg(long, default_value_t = 3)]
        max_cycles: usize,

        /// Apply safe Rust-owned efficiency mitigations in the target repo.
        #[arg(long)]
        self_heal: bool,

        /// Maximum orchestrator-repo cycles to run for unresolved efficiency alerts per target iteration.
        #[arg(long, default_value_t = 1)]
        orchestrator_cycles: usize,
    },

    /// Record repo change observations without running AI roles or Worker tasks.
    Observe {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Also mine recent Git commits into advisory observation indexes.
        #[arg(long)]
        history: bool,

        /// Maximum commits to scan when --history is set.
        #[arg(long, default_value_t = 200)]
        max_commits: usize,

        /// Directory or JSONL file containing user-provided Codex session exports.
        #[arg(long)]
        codex_sessions: Option<PathBuf>,
    },

    /// Preflight role providers and local backends (Ollama / MLX LM).
    Providers {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },

    /// Promote local observation memory into global advisory memory.
    Learn {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Merge local session/git observations into the configured global observation store.
        #[arg(long = "global")]
        global_memory: bool,
    },

    /// Start the pinned-repository MCP stdio server.
    Mcp {
        /// Repository/project root.
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Expose observation only, or explicitly enable supervised operations.
        #[arg(long, value_enum, default_value_t = McpAccessArg::Observe)]
        access: McpAccessArg,

        /// Maximum lifetime of one asynchronous step job.
        #[arg(
            long,
            default_value_t = crate::mcp::server::default_job_timeout_seconds()
        )]
        job_timeout_seconds: u64,
    },
}

#[derive(Subcommand, Debug)]
enum RecipeCommands {
    /// List repository proposals and applicable approved recipes.
    List {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// Include every machine-global approved recipe, not only applicable ones.
        #[arg(long = "global")]
        include_global: bool,
        #[arg(long)]
        json: bool,
    },
    /// Preview one exact repository-local proposal without mutation.
    Preview {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Validate in isolation and approve one exact revision machine-globally.
    Approve {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        expected_digest: String,
        #[arg(long)]
        expected_revision: String,
        #[arg(long, value_enum, default_value_t = RecipeApprovalScopeArg::Global)]
        scope: RecipeApprovalScopeArg,
        #[arg(long)]
        json: bool,
    },
    /// Reject one exact proposal without granting authority.
    Reject {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        expected_digest: String,
        #[arg(long)]
        json: bool,
    },
    /// Reactivate an approved revision after repository-local review.
    Reactivate {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        expected_digest: String,
        #[arg(long)]
        expected_revision: String,
        #[arg(long)]
        json: bool,
    },
    /// Retire one exact approved revision machine-globally.
    Retire {
        #[arg(long)]
        id: String,
        #[arg(long)]
        expected_digest: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum SelfImproveCommands {
    /// Review an exact evaluated knowledge candidate before publication.
    Review {
        #[arg(long)]
        vault: Option<PathBuf>,
        #[arg(long)]
        objective: String,
        #[arg(long)]
        json: bool,
    },
    /// Promote exactly the reviewed candidate and evaluation receipts.
    Promote {
        #[arg(long)]
        vault: Option<PathBuf>,
        #[arg(long)]
        objective: String,
        #[arg(long)]
        expected_review_digest: String,
        #[arg(long)]
        json: bool,
    },
    /// Exact-digest enrollment of clean source repositories and the login service.
    Enroll {
        #[arg(long)]
        install_login_service: bool,
        #[arg(long)]
        ca_repo: PathBuf,
        #[arg(long)]
        orchestrator_repo: PathBuf,
        /// Explicit named ca profiles whose sanitized sessions may feed learning.
        #[arg(long = "ca-profile", action = clap::ArgAction::Append)]
        ca_profiles: Vec<String>,
        /// Named ca profile used only for restricted role-run generation and replay.
        #[arg(long, default_value = "codex-team")]
        ca_execution_profile: String,
        #[arg(long)]
        confirm: String,
    },
    /// Preview a repository-independent knowledge-only enrollment contract.
    KnowledgePreview {
        #[arg(long)]
        supervisor_binary: PathBuf,
        #[arg(long)]
        ca_binary: PathBuf,
        #[arg(long)]
        codex_team_binary: PathBuf,
        #[arg(long = "ca-profile", action = clap::ArgAction::Append)]
        ca_profiles: Vec<String>,
        #[arg(long)]
        ca_execution_profile: String,
        #[arg(long = "evidence-root", action = clap::ArgAction::Append)]
        evidence_roots: Vec<String>,
        #[arg(long)]
        install_login_service: bool,
    },
    /// Apply one exact repository-independent knowledge-only enrollment.
    KnowledgeEnroll {
        #[arg(long)]
        supervisor_binary: PathBuf,
        #[arg(long)]
        ca_binary: PathBuf,
        #[arg(long)]
        codex_team_binary: PathBuf,
        #[arg(long = "ca-profile", action = clap::ArgAction::Append)]
        ca_profiles: Vec<String>,
        #[arg(long)]
        ca_execution_profile: String,
        #[arg(long = "evidence-root", action = clap::ArgAction::Append)]
        evidence_roots: Vec<String>,
        #[arg(long)]
        install_login_service: bool,
        #[arg(long)]
        confirm: String,
    },
    /// Report current learning, evaluation, usage, and A/B slot state.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Validate the vault, stable supervisor, and known-good pair.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Persist an operator-owned global pause.
    Pause {
        #[arg(long)]
        reason: String,
        #[arg(long)]
        json: bool,
    },
    /// Resume only from the exact reviewed state revision.
    Resume {
        #[arg(long)]
        expected_revision: String,
        #[arg(long)]
        json: bool,
    },
    /// Restore one component's prior independently verified binary slot.
    Rollback {
        #[arg(long, value_enum)]
        component: SelfImproveComponentArg,
        #[arg(long)]
        json: bool,
    },
    /// List immutable improvement objectives.
    Objectives {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List structurally scrubbed learning traces.
    Traces {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List immutable deployment receipts.
    Deployments {
        #[arg(long)]
        json: bool,
    },
    /// Internal one-stage worker invoked only by the stable supervisor.
    #[command(hide = true)]
    Worker {
        #[arg(long)]
        vault: PathBuf,
        #[arg(long)]
        expected_revision: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SelfImproveComponentArg {
    Ca,
    CodexTeam,
}

impl SelfImproveComponentArg {
    fn as_contract_value(self) -> crate::self_improvement::ComponentV1 {
        match self {
            Self::Ca => crate::self_improvement::ComponentV1::Ca,
            Self::CodexTeam => crate::self_improvement::ComponentV1::CodexTeam,
        }
    }
}

#[derive(Debug, Subcommand)]
enum AutomationCommands {
    Workflow {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        state_root: PathBuf,
        /// Reuse this identity to inspect/reconcile a duplicate legacy dispatch.
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long, default_value = "-")]
        input: PathBuf,
        #[arg(long)]
        json: bool,
    },
    WorkflowRoots {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Setup {
        #[arg(long)]
        profile_config: PathBuf,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        repo: PathBuf,
        #[arg(long, default_value = "-")]
        input: PathBuf,
        #[arg(long)]
        json: bool,
    },
}
fn learning_vault(path: Option<PathBuf>) -> Result<crate::self_improvement::LearningVault> {
    use anyhow::Context;
    match path {
        Some(path) => crate::self_improvement::LearningVault::existing_at(path)?,
        None => crate::self_improvement::LearningVault::existing_machine()?,
    }
    .context("self-improvement is not enrolled")
}

#[derive(Debug, Subcommand)]
enum BrokerCommands {
    #[command(hide = true, name = "internal-serve", alias = "serve")]
    Serve {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        owner_nonce: String,
    },
    Status {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Stop {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        json: bool,
    },
}
#[derive(Debug, Subcommand)]
enum RemoteCommands {
    Execute {
        #[arg(long)]
        config: PathBuf,
        #[arg(long, default_value = "-")]
        input: PathBuf,
        #[arg(long)]
        json: bool,
    },
}
#[derive(Debug, Subcommand)]
enum WorkflowCommands {
    Execute {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        state_root: PathBuf,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long, default_value = "-")]
        input: PathBuf,
        #[arg(long)]
        json: bool,
    },
}
#[derive(Debug, Subcommand)]
enum ProtectedBaselineCommands {
    Preview {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Apply {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        expected_revision: String,
        #[arg(long)]
        accept_digest: String,
        #[arg(long)]
        json: bool,
    },
}

fn read_request<T: serde::de::DeserializeOwned>(input: &std::path::Path) -> Result<T> {
    use std::io::Read;
    let mut bytes = Vec::new();
    let reader: Box<dyn Read> = if input == std::path::Path::new("-") {
        Box::new(std::io::stdin())
    } else {
        Box::new(std::fs::File::open(input)?)
    };
    reader.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 1024 * 1024, "request exceeds one MiB");
    Ok(serde_json::from_slice(&bytes)?)
}
fn print_receipt(receipt: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(receipt)?);
    Ok(())
}
fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?)
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RecipeApprovalScopeArg {
    Global,
}

#[derive(Clone, Debug, ValueEnum)]
enum DispositionDecisionArg {
    Preserve,
    Recheck,
    Gate,
    KeepPaused,
}

#[derive(Clone, Debug, ValueEnum)]
enum CommitModeArg {
    Auto,
    Manual,
}

#[derive(Clone, Debug, ValueEnum)]
enum VerifyCheckArg {
    SnapshotHygiene,
    ProtectedTrees,
    ReleaseReadiness,
}

impl VerifyCheckArg {
    fn as_str(&self) -> &'static str {
        match self {
            Self::SnapshotHygiene => "snapshot-hygiene",
            Self::ProtectedTrees => "protected-trees",
            Self::ReleaseReadiness => "release-readiness",
        }
    }
}

#[derive(Clone, Debug, ValueEnum)]
enum McpAccessArg {
    Observe,
    Delegate,
    Operate,
}

impl McpAccessArg {
    fn as_server_value(&self) -> crate::mcp::server::McpAccess {
        match self {
            Self::Observe => crate::mcp::server::McpAccess::Observe,
            Self::Delegate => crate::mcp::server::McpAccess::Delegate,
            Self::Operate => crate::mcp::server::McpAccess::Operate,
        }
    }
}

impl CommitModeArg {
    fn as_config_value(&self) -> crate::config::CommitMode {
        match self {
            Self::Auto => crate::config::CommitMode::Auto,
            Self::Manual => crate::config::CommitMode::Manual,
        }
    }
}

#[derive(Clone, Debug, ValueEnum)]
enum RoleArg {
    #[value(alias = "ceo")]
    Master,
    #[value(alias = "pm", alias = "project-manager")]
    Planner,
    #[value(alias = "senior", alias = "senior-dev")]
    Foreman,
    Worker,
}

impl RoleArg {
    fn as_config_key(&self) -> &'static str {
        match self {
            Self::Master => "master",
            Self::Planner => "planner",
            Self::Foreman => "foreman",
            Self::Worker => "worker",
        }
    }
}

impl DispositionDecisionArg {
    fn as_app_arg(&self) -> &'static str {
        match self {
            Self::Preserve => "preserve",
            Self::Recheck => "recheck",
            Self::Gate => "gate",
            Self::KeepPaused => "keep-paused",
        }
    }
}

pub fn run() -> Result<()> {
    match Cli::parse().command {
        Commands::Automation { command } => match command {
            AutomationCommands::Workflow {
                config,
                state_root,
                run_id,
                input,
                json: _,
            } => print_receipt(&runtime()?.block_on(
                crate::automation_workflow::execute_with_run_id(
                    &config,
                    &state_root,
                    read_request(&input)?,
                    run_id,
                ),
            )?),
            AutomationCommands::WorkflowRoots { config, json: _ } => print_receipt(
                &serde_json::json!({"schema":"codex-team.workflow_local_roots.v1", "roots":crate::remote::workflow_local_roots(&config)?}),
            ),
            AutomationCommands::Setup {
                profile_config,
                profile,
                repo,
                input,
                json: _,
            } => print_receipt(&crate::automation_setup::execute(
                &profile_config,
                &profile,
                &repo,
                &read_request(&input)?,
            )?),
        },
        Commands::Broker { command } => match command {
            BrokerCommands::Serve { repo, owner_nonce } => crate::broker::serve(repo, owner_nonce),
            BrokerCommands::Status { repo, json: _ } => {
                print_receipt(&crate::broker::status(&repo)?)
            }
            BrokerCommands::Stop { repo, json: _ } => print_receipt(&crate::broker::stop(&repo)?),
        },
        Commands::Remote {
            command:
                RemoteCommands::Execute {
                    config,
                    input,
                    json: _,
                },
        } => {
            let request = read_request(&input)?;
            print_receipt(&runtime()?.block_on(crate::remote::execute_file(&config, request))?)
        }
        Commands::Workflow {
            command:
                WorkflowCommands::Execute {
                    config,
                    state_root,
                    run_id,
                    input,
                    json: _,
                },
        } => {
            let request = read_request(&input)?;
            print_receipt(
                &runtime()?.block_on(crate::automation_workflow::execute_with_run_id(
                    &config,
                    &state_root,
                    request,
                    run_id,
                ))?,
            )
        }
        Commands::Mechanistic {
            repo,
            input,
            json: _,
        } => {
            let request: ca_automation_protocol::MechanisticRequestV1 = read_request(&input)?;
            let executor = crate::mechanistic::MechanisticExecutor::new(&repo)?;
            let receipt = if request.actions.iter().all(|action| action.is_query()) {
                executor.execute_queries(&request)?
            } else {
                executor.execute_evidence(&request)?
            };
            print_receipt(&receipt)
        }
        Commands::ProtectedBaseline { command } => match command {
            ProtectedBaselineCommands::Preview { repo, json: _ } => print_receipt(
                &crate::verification::protected_tree::preview(&crate::paths::Paths::new(&repo)?)?,
            ),
            ProtectedBaselineCommands::Apply {
                repo,
                expected_revision,
                accept_digest,
                json: _,
            } => print_receipt(&crate::verification::protected_tree::apply(
                &crate::paths::Paths::new(&repo)?,
                &expected_revision,
                &accept_digest,
            )?),
        },
        Commands::Init {
            repo,
            mission,
            commit_mode,
            require_clean_start,
        } => crate::app::init(
            repo,
            mission,
            commit_mode.map(|mode| mode.as_config_value()),
            require_clean_start,
        ),
        Commands::CommitPending {
            repo,
            id,
            expected_digest,
            expected_revision,
            json,
        } => crate::app::commit_pending(repo, id, expected_digest, expected_revision, json),
        Commands::TaskImport {
            repo,
            input,
            accept_digest,
            expected_revision,
            json,
        } => crate::app::task_import(repo, input, accept_digest, expected_revision, json),
        Commands::Recipe { command } => match command {
            RecipeCommands::List {
                repo,
                include_global,
                json,
            } => crate::app::recipe_list(repo, include_global, json),
            RecipeCommands::Preview { repo, id, json } => {
                crate::app::recipe_preview(repo, id, json)
            }
            RecipeCommands::Approve {
                repo,
                id,
                expected_digest,
                expected_revision,
                scope: RecipeApprovalScopeArg::Global,
                json,
            } => crate::app::recipe_approve(repo, id, expected_digest, expected_revision, json),
            RecipeCommands::Reject {
                repo,
                id,
                expected_digest,
                json,
            } => crate::app::recipe_reject(repo, id, expected_digest, json),
            RecipeCommands::Reactivate {
                repo,
                id,
                expected_digest,
                expected_revision,
                json,
            } => crate::app::recipe_reactivate(repo, id, expected_digest, expected_revision, json),
            RecipeCommands::Retire {
                id,
                expected_digest,
                json,
            } => crate::app::recipe_retire(id, expected_digest, json),
        },
        Commands::SelfImprove { command } => match command {
            SelfImproveCommands::Review {
                vault,
                objective,
                json: _,
            } => print_receipt(&crate::self_improvement::preview_learning_promotion(
                &learning_vault(vault)?,
                &objective,
            )?),
            SelfImproveCommands::Promote {
                vault,
                objective,
                expected_review_digest,
                json: _,
            } => {
                let vault = learning_vault(vault)?;
                let bridge = crate::self_improvement::ProcessCaLearningBridgeClient {
                    binary: crate::self_improvement::active_component_binary(&vault, "ca")?,
                };
                let revision = crate::self_improvement::promote_learning_candidate(
                    &vault,
                    &bridge,
                    &objective,
                    &expected_review_digest,
                )?;
                print_receipt(
                    &serde_json::json!({"schema":"ca.learning-promotion.v2", "revision":revision}),
                )
            }
            SelfImproveCommands::Enroll {
                install_login_service,
                ca_repo,
                orchestrator_repo,
                ca_profiles,
                ca_execution_profile,
                confirm,
            } => crate::app::self_improve_enroll(
                ca_repo,
                orchestrator_repo,
                install_login_service,
                ca_profiles,
                ca_execution_profile,
                confirm,
            ),
            SelfImproveCommands::KnowledgePreview {
                supervisor_binary,
                ca_binary,
                codex_team_binary,
                ca_profiles,
                ca_execution_profile,
                evidence_roots,
                install_login_service,
            } => crate::app::self_improve_knowledge_preview(
                supervisor_binary,
                ca_binary,
                codex_team_binary,
                ca_profiles,
                ca_execution_profile,
                evidence_roots,
                install_login_service,
            ),
            SelfImproveCommands::KnowledgeEnroll {
                supervisor_binary,
                ca_binary,
                codex_team_binary,
                ca_profiles,
                ca_execution_profile,
                evidence_roots,
                install_login_service,
                confirm,
            } => crate::app::self_improve_knowledge_enroll(
                supervisor_binary,
                ca_binary,
                codex_team_binary,
                ca_profiles,
                ca_execution_profile,
                evidence_roots,
                install_login_service,
                confirm,
            ),
            SelfImproveCommands::Status { json } => crate::app::self_improve_status(json),
            SelfImproveCommands::Doctor { json } => crate::app::self_improve_doctor(json),
            SelfImproveCommands::Pause { reason, json } => {
                crate::app::self_improve_pause(reason, json)
            }
            SelfImproveCommands::Resume {
                expected_revision,
                json,
            } => crate::app::self_improve_resume(expected_revision, json),
            SelfImproveCommands::Rollback { component, json } => {
                crate::app::self_improve_rollback(component.as_contract_value(), json)
            }
            SelfImproveCommands::Objectives { id, json } => {
                crate::app::self_improve_objectives(id, json)
            }
            SelfImproveCommands::Traces { id, json } => crate::app::self_improve_traces(id, json),
            SelfImproveCommands::Deployments { json } => crate::app::self_improve_deployments(json),
            SelfImproveCommands::Worker {
                vault,
                expected_revision,
            } => crate::app::self_improve_worker(vault, expected_revision),
        },
        Commands::Verify {
            check: VerifyCheckArg::ReleaseReadiness,
            repo,
            json,
        } => crate::app::verify_release_readiness(repo, json),
        Commands::Verify { check, repo, json } => {
            crate::app::verify_builtin(repo, check.as_str().to_string(), json)
        }
        Commands::Step {
            repo,
            json,
            expected_revision,
        } => {
            crate::mcp::server::await_step_launch_barrier()?;
            crate::app::step_with_expected_revision(repo, json, expected_revision)
        }
        Commands::Run {
            repo,
            max_cycles,
            pause_on_escalation,
            auto_continue,
        } => crate::app::run_cycles(repo, max_cycles, pause_on_escalation, auto_continue),
        Commands::Status {
            repo,
            json,
            observe,
        } => crate::app::status(repo, json, observe),
        Commands::Mission { repo, mission } => crate::app::set_mission(repo, mission),
        Commands::Models {
            repo,
            role,
            model,
            effort,
            timeout_seconds,
        } => crate::app::models(
            repo,
            role.map(|role| role.as_config_key().to_string()),
            model,
            effort,
            timeout_seconds,
        ),
        Commands::Disposition {
            repo,
            decision,
            refresh_report,
            json,
            expected_revision,
            pause_fingerprint,
        } => crate::app::disposition_with_preconditions(
            repo,
            decision.as_app_arg().to_string(),
            refresh_report,
            json,
            expected_revision,
            pause_fingerprint,
        ),
        Commands::Cost { repo } => crate::app::cost(repo),
        Commands::RouteBench { repo, json } => crate::app::route_bench(repo, json),
        Commands::Doctor { repo, json } => crate::app::doctor_command(repo, json),
        Commands::Providers { repo, json } => crate::app::providers_command(repo, json),
        Commands::Commands { repo, task } => crate::app::export_commands(repo, task),
        Commands::Watch {
            repo,
            task,
            self_heal,
        } => crate::app::watch(repo, task, self_heal),
        Commands::WatchLoop {
            repo,
            orchestrator_repo,
            orchestrator_mission,
            max_cycles,
            self_heal,
            orchestrator_cycles,
        } => crate::app::watch_loop(
            repo,
            orchestrator_repo,
            orchestrator_mission,
            max_cycles,
            self_heal,
            orchestrator_cycles,
        ),
        Commands::Observe {
            repo,
            history,
            max_commits,
            codex_sessions,
        } => crate::app::observe(repo, history, max_commits, codex_sessions),
        Commands::Learn {
            repo,
            global_memory,
        } => crate::app::learn(repo, global_memory),
        Commands::Mcp {
            repo,
            access,
            job_timeout_seconds,
        } => crate::mcp::server::serve_with_options(
            repo,
            access.as_server_value(),
            job_timeout_seconds,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compatibility_parser_exposes_native_typed_surfaces() {
        for arguments in [
            vec!["codex-team", "mcp", "--access", "delegate"],
            vec![
                "codex-team",
                "remote",
                "execute",
                "--config",
                "/fixture/remote.toml",
                "--json",
            ],
            vec![
                "codex-team",
                "automation",
                "workflow",
                "--config",
                "/fixture/remote.toml",
                "--state-root",
                "/fixture/state",
                "--json",
            ],
            vec![
                "codex-team",
                "automation",
                "workflow-roots",
                "--config",
                "/fixture/remote.toml",
                "--json",
            ],
            vec!["codex-team", "protected-baseline", "preview", "--json"],
            vec!["codex-team", "broker", "status", "--json"],
        ] {
            assert!(Cli::try_parse_from(&arguments).is_ok(), "{arguments:?}");
        }
    }
}
