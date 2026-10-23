#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandId {
    Feedback,
    Model,
    Preset,
    PresetSave,
    Provider,
    AddDir,
    Workspace,
    SaveWorkspace,
    LoadWorkspace,
    Rule,
    CommandManager,
    Plugin,
    Direnv,
    Skills,
    Bundles,
    Recall,
    Learning,
    Mcp,
    Hooks,
    Permissions,
    Memory,
    Personality,
    Learn,
    Dream,
    Distill,
    Goal,
    Workflow,
    Automation,
    Worktree,
    MultiRun,
    Reasoning,
    Thinking,
    Plan,
    Recipes,
    SelfImprove,
    Tasks,
    Agents,
    Ask,
    Btw,
    Voice,
    Debug,
    Logs,
    Errors,
    SupportBundle,
    Yolo,
    AutoReview,
    New,
    Clear,
    Quit,
    Help,
    Vim,
    Version,
    Mode,
    Config,
    Sandbox,
    ContextLimit,
    PromptCache,
    Doctor,
    Audit,
    Raw,
    Output,
    Scratch,
    Copy,
    Diff,
    Walkthrough,
    Review,
    Acceptance,
    Artifacts,
    Split,
    Pane,
    Save,
    Resume,
    Retention,
    Export,
    Import,
    Init,
    Compact,
    Summarize,
    Shell,
    Commit,
    Undo,
    Redo,
    Recover,
    Branches,
    Bookmark,
    Pause,
    Unpause,
    Fork,
    Rewind,
    Retry,
    Timeline,
    Queue,
    Context,
    Session,
    Usage,
    Dashboard,
    Request,
    Trace,
    Filter,
    Keymap,
    History,
    Stash,
    Jobs,
    Delegate,
    Evidence,
    Term,
    Theme,
    SyncTheme,
    StatusIndicators,
    Statusline,
    CompactMode,
    LineNumbers,
    Reload,
    Name,
    Connect,
    Disconnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCategory {
    General,
    Mode,
    Session,
    Workspace,
    Display,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAvailability {
    Always,
    IdleOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationClass {
    ReadOnly,
    State,
    Workspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSelectionBehavior {
    Execute,
    CompleteArguments,
}

impl CommandId {
    pub const fn selection_behavior(self) -> CommandSelectionBehavior {
        match self {
            Self::PresetSave
            | Self::AddDir
            | Self::SaveWorkspace
            | Self::LoadWorkspace
            | Self::Output
            | Self::Import
            | Self::Name => CommandSelectionBehavior::CompleteArguments,
            _ => CommandSelectionBehavior::Execute,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    pub id: CommandId,
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub usage: &'static str,
    pub category: CommandCategory,
    pub availability: CommandAvailability,
    pub mutation: MutationClass,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandAction {
    pub id: CommandId,
    pub args: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputAction {
    Command(CommandAction),
    SelectModel(crate::app::ModelInfo),
    SelectReasoning(crate::config::ReasoningEffort),
    ToggleModelFavorite(crate::app::ModelInfo),
    SelectReviewModel(crate::app::ModelInfo),
    SaveDelegation(Option<crate::config::DelegationRoute>),
    SaveDelegationMode(crate::config::DelegationMode),
    OpenProviderEditor(crate::app::ProviderEditorRequest),
    SaveProviderModel(crate::app::ProviderModelDraft),
    SaveProvider(crate::app::ProviderModelDraft),
    SaveSandboxMode(crate::sandbox::SandboxMode),
    SaveContextLimits(crate::config::ContextLimitUpdate),
    SaveModelPromptCache(crate::config::ModelPromptCacheUpdate),
    SaveKeymapBinding {
        action: crate::keymap::KeyAction,
        chord: Option<crate::keymap::KeyChord>,
    },
    SavePromptDraft,
    RestorePromptDraft(uuid::Uuid),
    DeletePromptDraft(uuid::Uuid),
    DeletePermission(crate::app::PermissionEntrySource),
    SaveMemoryRecord {
        id: Option<uuid::Uuid>,
        scope: crate::memory::MemoryScope,
        kind: crate::memory::MemoryKind,
        text: String,
    },
    DeleteMemoryRecord {
        scope: crate::memory::MemoryScope,
        id: uuid::Uuid,
    },
    ApproveMemoryProposal(uuid::Uuid),
    SaveMemorySettings {
        scope: crate::memory::MemoryScope,
        recall: bool,
        generation: bool,
    },
    ExportMemory(std::path::PathBuf),
    SavePersonality {
        revision: String,
        preferences: crate::personality_preferences::Preferences,
    },
    StartAgentGraph,
    PauseAgentGraph,
    CancelAgentGraph,
    RetryAgentTask(crate::work::TaskId),
    ReviewAgentTask(crate::work::TaskId),
    StartMultiRun,
    CancelMultiRun,
    RefreshMultiRunEvidence,
    JudgeMultiRun,
    FuseMultiRun,
    ResolveMultiRunApproval {
        candidate: crate::multi_run::CandidateId,
        approved: bool,
    },
    SelectMultiRunRoute {
        candidate: crate::multi_run::CandidateId,
        model: crate::app::ModelInfo,
    },
    PersistSessionQueue,
    RequestDeleteProvider(String),
    DeleteProvider {
        provider: String,
        replacement: Option<crate::app::ModelInfo>,
    },
    CheckProvider(String),
    CancelProviderCheck,
    CancelBtw,
    StartVoice,
    StopVoice,
    CancelVoice,
    SelectCheckedProvider(crate::app::ModelInfo),
    LoadSession(String),
    NewSession,
    DeleteSession {
        storage_key: String,
        expected_id: uuid::Uuid,
        archived: bool,
    },
    DeleteTrace(uuid::Uuid),
    CreateProjectInstructions(crate::project_init::ProjectInitDraft),
    CancelProjectInit,
    SetSessionPinned {
        storage_key: String,
        expected_id: uuid::Uuid,
        pinned: bool,
        generation: u64,
    },
    TransferSession {
        storage_key: String,
        expected_id: uuid::Uuid,
        direction: crate::app::SessionTransferDirection,
        generation: u64,
    },
    RenameSession {
        storage_key: String,
        expected_id: uuid::Uuid,
        title: String,
        archived: bool,
        generation: u64,
    },
    GenerateSessionTitle {
        storage_key: String,
        expected_id: uuid::Uuid,
        archived: bool,
        generation: u64,
    },
    BranchAt {
        through_message_count: usize,
        branch_turn: usize,
    },
    Steer {
        text: String,
        queued_id: Option<uuid::Uuid>,
    },
    ResumeLoopGuard,
    StopTurn,
    OpenChangeDiff(crate::changes::ChangeFile),
    RefreshChangeReview,
    StageChange(crate::changes::ChangeFile),
    UnstageChange(crate::changes::ChangeFile),
    PrepareCommit(String),
    CommitChanges {
        snapshot: crate::changes::GitCommitSnapshot,
        subject: String,
    },
    CancelChangeOperation,
    CancelChangeWalkthrough,
    KillJob(crate::jobs::JobId),
    StopWatcher(ca_watch::WatchId),
    CancelDelegation(String),
    FeedbackDelegation {
        job_id: String,
        category: crate::mechanistic_delegation::DelegationFeedbackCategory,
        correction_confirmed: bool,
    },
    RefreshTerminals,
    CreateTerminal(String),
    AttachTerminal(String),
    DetachTerminal(String),
    KillTerminal(String),
    SaveTheme(crate::ui_settings::UiTheme),
    SaveNotification(crate::ui_settings::NotificationStyle),
    SaveMouse(crate::ui_settings::MouseMode),
    SaveCommandOutput(crate::config::CommandOutputPolicy),
    RerunFullCommand(crate::command_output::FullOutputRerun),
    DeleteScratch(uuid::Uuid),
    ApplyRoutePreset(String),
    SaveRoutePreset {
        name: String,
        preset: crate::config::RoutePreset,
    },
    SaveStatusline(crate::ui_settings::StatuslineUpdate),
    TestStatusCommand,
    CancelStatusCommand,
    OpenEditor,
    RefreshFilePicker,
    RefreshPathCompletion,
    RecordAttachmentFrecency(crate::attachments::FrecencyKey),
    AttachClipboardImage {
        image: crate::image_content::RawClipboardImage,
        generation: u64,
    },
    CycleReasoning,
    TrustWorkspace,
    RunLaunchWorktreeSetup,
    CreateSupportBundle,
    ApplySetupImport,
    StartCodexAuth,
    RevokeCodexAuth,
    StartProviderAuth(crate::provider_connect::FirstClassProvider),
    CopyProviderAuthUrl(String),
    SubmitProviderAuth,
    CancelProviderAuth,
    OpenLearningReviewActions,
    RefreshLearningReview,
    ConfirmLearningProposal,
    DeclineLearningProposal,
}

macro_rules! command {
    ($id:ident, $name:literal, [$($alias:literal),*], $description:literal, $usage:literal, $category:ident, $availability:ident, $mutation:ident, $visible:literal) => {
        CommandSpec { id: CommandId::$id, name: $name, aliases: &[$($alias),*], description: $description, usage: $usage, category: CommandCategory::$category, availability: CommandAvailability::$availability, mutation: MutationClass::$mutation, visible: $visible }
    };
}

pub static COMMANDS: &[CommandSpec] = &[
    command!(
        Model,
        "model",
        ["m"],
        "Switch model",
        "[model-id] [--once]",
        General,
        Always,
        State,
        true
    ),
    command!(
        Provider,
        "provider",
        ["providers"],
        "Manage provider templates, providers, and models",
        "[templates|setup|discover|…]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Preset,
        "preset",
        [],
        "Apply a named route preset",
        "[name]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        PresetSave,
        "preset-save",
        [],
        "Save the current route as a named preset",
        "<name>",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        AddDir,
        "add-dir",
        [],
        "Grant an additional session workspace root",
        "<path>",
        Workspace,
        IdleOnly,
        State,
        true
    ),
    command!(
        Workspace,
        "workspace",
        ["roots"],
        "List or manage named workspaces and session roots",
        "[list|save <name>|load <name>|delete <name>|cd <path>|remove <root-id>]",
        Workspace,
        IdleOnly,
        State,
        true
    ),
    command!(
        SaveWorkspace,
        "save-workspace",
        [],
        "Save the current ordered workspace roots by name",
        "<name>",
        Workspace,
        IdleOnly,
        State,
        true
    ),
    command!(
        LoadWorkspace,
        "load-workspace",
        [],
        "Load a named workspace into the current session",
        "<name>",
        Workspace,
        IdleOnly,
        State,
        true
    ),
    command!(
        Rule,
        "rule",
        ["rules"],
        "Browse and manage conditional user or workspace rules",
        "[filter|view <id>|activate <id>|deactivate <id>|create …|edit …|delete <id>|confirm <digest>|reload]",
        Workspace,
        IdleOnly,
        State,
        true
    ),
    command!(
        CommandManager,
        "command",
        ["commands"],
        "Browse and manage native custom prompt commands",
        "[filter|view <name>|create …|edit …|delete <name>|confirm <digest>|reload]",
        Workspace,
        IdleOnly,
        State,
        true
    ),
    command!(
        Plugin,
        "plugin",
        ["plugins"],
        "Browse and manage verified local plugins and signed catalogs",
        "[list|search <query>|show <id>|add <manifest>|enable <id>|disable <id>|remove <id>|catalog <action>]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Direnv,
        "direnv",
        [],
        "Preview and approve a process-only workspace environment overlay",
        "[confirm <sha256>]",
        Workspace,
        IdleOnly,
        State,
        true
    ),
    command!(
        Skills,
        "skills",
        [],
        "Search and manage filesystem skills",
        "[query|activate <id>|deactivate <id>|view <id>|unavailable|config <selector> [<key>=<value>] [--workspace]|reload]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Bundles,
        "bundles",
        [],
        "List, inspect, create, delete, or reload task-local skill bundles",
        "[show <name>|create <name> <skill1> <skill2> [...skill5] [--workspace]|delete <name>|reload]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Recall,
        "recall",
        [],
        "Search privacy-filtered completed CA, Codex, Hermes, and MiMo sessions locally",
        "[query|read <session-ref> [cursor]]",
        General,
        IdleOnly,
        ReadOnly,
        true
    ),
    command!(
        Learning,
        "learning",
        ["journey"],
        "Inspect the metadata-only learning and reusable-asset lifecycle",
        "[skills|memory|runs|graph|suggestions|skill <selector>|<event-id>]",
        General,
        IdleOnly,
        ReadOnly,
        true
    ),
    command!(
        Mcp,
        "mcp",
        [],
        "Inspect, authorize, or reconnect configured MCP servers",
        "[verbose|connect <server>|refresh <server>|disconnect <server>|reconnect <server>|trust-project [--network]|allow-network <server>]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Hooks,
        "hooks",
        [],
        "Inspect, test, and toggle lifecycle hooks",
        "[filter]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Permissions,
        "permissions",
        ["permission"],
        "Inspect permission rules and recent decisions",
        "[filter|why]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Memory,
        "memory",
        [],
        "Inspect and manage opt-in user or workspace memory",
        "[filter|export <path>]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Learn,
        "learn",
        [],
        "Learn directly from recent work and publish validated reusable assets",
        "[focus text|status [run-id]|sync|undo <run-id>]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Personality,
        "personality",
        ["questionnaire"],
        "Answer guided questions to customize how CA works and communicates",
        "",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Dream,
        "dream",
        [],
        "Manage conservative memory maintenance",
        "[run|status|on|off|undo <run-id>]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Distill,
        "distill",
        [],
        "Distill reusable assets directly without changing memory",
        "[focus text|run [focus]|status|on|off|undo <run-id>]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Goal,
        "goal",
        [],
        "Set or manage a finite evidence-evaluated goal session",
        "[status|pause|resume|clear|<condition> [--iterations N] [--tokens N] [--minutes N]]",
        General,
        Always,
        ReadOnly,
        true
    ),
    command!(
        Workflow,
        "workflow",
        ["workflows"],
        "Run, inspect, or manage finite declarative and audited built-in workflows",
        "[templates|inspect builtin:<id>|run <path|builtin:id> [options]|tree [builtin:<id>]|status|pause|resume|clear]",
        General,
        Always,
        State,
        true
    ),
    command!(
        Automation,
        "automation",
        ["automations"],
        "Inspect local automations and review an exact definition before enabling it",
        "[list|inspect ID|history [ID]|pause ID|resume ID|run ID|result ID|preview FILE|create TOKEN|update ID TOKEN]",
        General,
        Always,
        State,
        true
    ),
    command!(
        Worktree,
        "worktree",
        ["worktrees"],
        "Create, inspect, attach, or clean up isolated Git worktrees",
        "[list|create <label> [base]|status <id>|diff <id>|attach <id>|cleanup <id>]",
        Workspace,
        Always,
        Workspace,
        true
    ),
    command!(
        MultiRun,
        "multi-run",
        ["best-of"],
        "Run one request in isolated candidate worktrees",
        "[2-5] [request]",
        General,
        IdleOnly,
        Workspace,
        true
    ),
    command!(
        Reasoning,
        "reasoning",
        [],
        "Show or select configured reasoning effort",
        "[none|minimal|low|medium|high|xhigh|max]",
        General,
        Always,
        State,
        true
    ),
    command!(
        Thinking,
        "thinking",
        [],
        "Toggle expanded reasoning display",
        "[on|off]",
        Display,
        Always,
        State,
        true
    ),
    command!(
        Plan,
        "plan",
        ["p"],
        "Plan, hand off across providers, inspect the workflow, or execute",
        "[show|run <request>|send [review|revise] [agent|provider/model]|graph|pause|resume|cancel|execute [provider/model|choose]|approve|revise|dismiss|reject|edit <title>|recipes]",
        Mode,
        IdleOnly,
        State,
        true
    ),
    command!(
        Recipes,
        "recipes",
        ["learned-recipes"],
        "Review learned Rust recipes without a model request",
        "[server] [recipe-id]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        SelfImprove,
        "self-improve",
        ["learning-fabric"],
        "Open machine learning, evaluation, deployment, pause, and rollback controls",
        "[server]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Tasks,
        "tasks",
        [],
        "Manage dependency-aware plan tasks and child agents",
        "[status|list|start|…]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Agents,
        "agents",
        [],
        "Inspect and control asynchronous child agents",
        "[pause|resume|cancel <id>|message <id> <text>|approve <id>|deny <id>]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Ask,
        "ask",
        ["a"],
        "Enter ask mode",
        "",
        Mode,
        Always,
        State,
        true
    ),
    command!(
        Btw,
        "btw",
        [],
        "Ask an ephemeral side question",
        "[question]",
        General,
        Always,
        State,
        true
    ),
    command!(
        Voice,
        "voice",
        [],
        "Record voice into an editable draft",
        "",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Debug,
        "debug",
        ["d"],
        "Enter debug mode",
        "",
        Mode,
        Always,
        State,
        true
    ),
    command!(
        Logs,
        "logs",
        [],
        "View safe runtime diagnostics",
        "[level=<info|warn|error>] [component=<name>] [event=<name>]",
        Display,
        Always,
        ReadOnly,
        true
    ),
    command!(
        Errors,
        "errors",
        [],
        "View recent safe provider failure categories",
        "[dismiss]",
        Display,
        Always,
        ReadOnly,
        true
    ),
    command!(
        SupportBundle,
        "support-bundle",
        [],
        "Preview and create a private metadata-only support archive",
        "[since=<duration>]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Yolo,
        "yolo",
        ["y", "re", "run-everything"],
        "Auto-approve all tools",
        "",
        Mode,
        Always,
        State,
        true
    ),
    command!(
        AutoReview,
        "auto-review",
        ["ar"],
        "Toggle safe-tool auto-review",
        "",
        Mode,
        Always,
        State,
        true
    ),
    command!(
        Mode,
        "mode",
        [],
        "Switch to a named agent mode",
        "<name>",
        Mode,
        IdleOnly,
        State,
        true
    ),
    command!(
        New,
        "new",
        [],
        "Start a new session; keep the previous one",
        "",
        Session,
        IdleOnly,
        State,
        true
    ),
    command!(
        Clear,
        "clear",
        ["c"],
        "Clear the visible conversation",
        "",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Quit,
        "quit",
        ["q", "exit"],
        "Quit ca",
        "",
        General,
        Always,
        State,
        true
    ),
    command!(
        Help,
        "help",
        ["h"],
        "Show command help",
        "",
        General,
        Always,
        ReadOnly,
        true
    ),
    command!(
        Vim,
        "vim",
        [],
        "Toggle Vim input mode",
        "",
        Display,
        Always,
        State,
        true
    ),
    command!(
        Version,
        "version",
        [],
        "Show ca version",
        "",
        General,
        Always,
        ReadOnly,
        true
    ),
    command!(
        Config,
        "config",
        [],
        "Search and edit settings/configuration",
        "[filter|explain [selector]]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Sandbox,
        "sandbox",
        [],
        "Inspect process isolation and choose the next-launch sandbox mode",
        "",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        ContextLimit,
        "context-limit",
        [],
        "Edit model context window and checkpoint threshold",
        "[provider/model]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        PromptCache,
        "prompt-cache",
        ["cache"],
        "Inspect or edit prompt caching for one exact model",
        "[provider/model]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Usage,
        "usage",
        [],
        "Show context, account, and session usage in a tabbed dashboard",
        "",
        General,
        Always,
        ReadOnly,
        true
    ),
    command!(
        Dashboard,
        "dashboard",
        ["agents-dashboard"],
        "Open the agent dashboard",
        "",
        Session,
        Always,
        ReadOnly,
        true
    ),
    command!(
        Request,
        "request",
        [],
        "Inspect the latest prepared model request",
        "",
        General,
        Always,
        ReadOnly,
        true
    ),
    command!(
        Trace,
        "trace",
        [],
        "Inspect or control persistent provider exchange tracing",
        "[on|off|list|open <id>|delete <id>]",
        General,
        Always,
        State,
        true
    ),
    command!(
        Filter,
        "filter",
        [],
        "Show the provider filter, or turn it on for this process",
        "[on]",
        General,
        Always,
        State,
        true
    ),
    command!(
        Keymap,
        "keymap",
        [],
        "Inspect or edit global keyboard shortcuts",
        "[filter]",
        Display,
        IdleOnly,
        State,
        true
    ),
    command!(
        History,
        "history",
        [],
        "Search process input history",
        "[filter]",
        General,
        Always,
        ReadOnly,
        true
    ),
    command!(
        Stash,
        "stash",
        [],
        "Save or restore explicit composer drafts",
        "[save]",
        General,
        Always,
        State,
        true
    ),
    command!(Feedback, "feedback", [], "Record explicit feedback for this task", "accepted|rejected|unknown", General, IdleOnly, State, true),
    command!(
        Doctor,
        "doctor",
        [],
        "Check local configuration and runtime readiness",
        "",
        General,
        IdleOnly,
        ReadOnly,
        true
    ),
    command!(
        Audit,
        "audit",
        [],
        "Audit local agent-harness security boundaries",
        "",
        General,
        IdleOnly,
        ReadOnly,
        true
    ),
    command!(
        Raw,
        "raw",
        [],
        "Open a copy-friendly transcript in native scrollback",
        "",
        General,
        IdleOnly,
        ReadOnly,
        true
    ),
    command!(
        Output,
        "output",
        [],
        "Rerun the latest semantic command with full native output",
        "full",
        General,
        IdleOnly,
        Workspace,
        true
    ),
    command!(
        Scratch,
        "scratch",
        [],
        "Manage project notes, todos, and saved plan references",
        "[note <text>|todo <text>|edit <id> <text>|toggle <id>|plan|delete <id>]",
        Workspace,
        IdleOnly,
        State,
        true
    ),
    command!(
        Copy,
        "copy",
        [],
        "Choose a prior message part to copy",
        "[last|all]",
        General,
        Always,
        State,
        true
    ),
    command!(
        Diff,
        "diff",
        ["changes"],
        "Review working-tree changes",
        "",
        Workspace,
        IdleOnly,
        ReadOnly,
        true
    ),
    command!(
        Walkthrough,
        "walkthrough",
        ["explain-changes"],
        "Explain one complete bounded Git snapshot",
        "[working|base <branch>|commit <sha>]",
        Workspace,
        IdleOnly,
        ReadOnly,
        true
    ),
    command!(
        Review,
        "review",
        [],
        "Run a fresh-context model review or explicit dual-route convergence",
        "[working|base <branch>|commit <sha>|custom <instructions>|converge [--ack-unpriced] [SCOPE]]",
        Workspace,
        IdleOnly,
        ReadOnly,
        true
    ),
    command!(
        Acceptance,
        "acceptance",
        [],
        "Create or inspect immutable convergent-review acceptance packets",
        "create|latest|list|export <FILE>",
        Workspace,
        IdleOnly,
        State,
        true
    ),
    command!(
        Artifacts,
        "artifacts",
        [],
        "Browse and inspect the current session's tracked files",
        "[search <query>|inspect|view|copy|untrack <number-or-path>]",
        Session,
        IdleOnly,
        State,
        true
    ),
    command!(
        Split,
        "split",
        [],
        "Open a bounded horizontal or vertical session pane",
        "horizontal|vertical [new|SESSION]",
        Session,
        Always,
        State,
        true
    ),
    command!(
        Pane,
        "pane",
        ["panes"],
        "Focus, close, or list interactive session panes",
        "next|prev|close|list|<number>",
        Session,
        Always,
        State,
        true
    ),
    command!(
        Save,
        "save",
        [],
        "Save the current session",
        "",
        Session,
        IdleOnly,
        State,
        true
    ),
    command!(
        Resume,
        "sessions",
        ["resume", "continue"],
        "Browse and continue previous sessions",
        "[session-id]",
        Session,
        IdleOnly,
        State,
        true
    ),
    command!(
        Retention,
        "retention",
        [],
        "Preview and manage saved-session retention",
        "",
        Session,
        IdleOnly,
        State,
        true
    ),
    command!(
        Export,
        "export",
        [],
        "Export the current session conversation",
        "[markdown|json] [path]",
        Session,
        IdleOnly,
        State,
        true
    ),
    command!(
        Import,
        "import",
        [],
        "Import portable or inert foreign session history",
        "[codex|claude|opencode] <path> | setup claude <directory>",
        Session,
        IdleOnly,
        State,
        true
    ),
    command!(
        Init,
        "init",
        [],
        "Draft root AGENTS.md with review",
        "",
        Workspace,
        IdleOnly,
        Workspace,
        true
    ),
    command!(
        Compact,
        "compact",
        [],
        "Compact conversation context",
        "[focus] | here [N]",
        Session,
        IdleOnly,
        State,
        true
    ),
    command!(
        Summarize,
        "summarize",
        [],
        "Show context estimate",
        "",
        Session,
        Always,
        ReadOnly,
        true
    ),
    command!(
        Shell,
        "shell",
        [],
        "Start a direct shell command",
        "",
        Workspace,
        IdleOnly,
        State,
        true
    ),
    command!(
        Commit,
        "commit",
        [],
        "Review staged changes and commit",
        "",
        Workspace,
        IdleOnly,
        Workspace,
        true
    ),
    command!(
        Undo,
        "undo",
        [],
        "Undo the last tracked file mutation",
        "",
        Workspace,
        IdleOnly,
        Workspace,
        true
    ),
    command!(
        Redo,
        "redo",
        [],
        "Redo the last undone file mutation",
        "",
        Workspace,
        IdleOnly,
        Workspace,
        true
    ),
    command!(Recover, "recover", [], "Preview and restore durable recorded turn edits", "[list|preview TURN_UUID|restore TOKEN]", Workspace, IdleOnly, Workspace, true),
    command!(Branches, "branches", ["tree"], "Browse local session branches and archived relatives", "[page OFFSET|switch SESSION_UUID]", Session, IdleOnly, State, true),
    command!(Bookmark, "bookmark", ["bookmarks"], "Save a named completed message boundary", "[list|add LABEL|rename UUID LABEL|delete UUID|branch UUID]", Session, IdleOnly, State, true),
    command!(
        Pause,
        "pause",
        [],
        "Pause the active turn at its next safe boundary",
        "",
        Session,
        Always,
        State,
        true
    ),
    command!(
        Unpause,
        "unpause",
        [],
        "Resume a turn paused at a safe boundary",
        "",
        Session,
        Always,
        State,
        true
    ),
    command!(
        Fork,
        "fork",
        [],
        "Fork the current conversation",
        "[title]",
        Session,
        IdleOnly,
        State,
        true
    ),
    command!(
        Rewind,
        "rewind",
        [],
        "Rewind the last conversation exchange",
        "",
        Session,
        IdleOnly,
        State,
        true
    ),
    command!(
        Retry,
        "retry",
        [],
        "Retry the failed or last user request in a new branch",
        "",
        Session,
        IdleOnly,
        State,
        true
    ),
    command!(
        Timeline,
        "timeline",
        [],
        "Browse completed conversation turns",
        "",
        Session,
        IdleOnly,
        State,
        true
    ),
    command!(
        Queue,
        "queue",
        [],
        "View and edit queued messages",
        "",
        General,
        Always,
        State,
        true
    ),
    command!(
        Context,
        "context",
        [],
        "Show context usage and checkpoint coverage",
        "",
        Session,
        Always,
        ReadOnly,
        true
    ),
    command!(
        Session,
        "session",
        [],
        "Show conversation history and current-sitting metrics",
        "",
        Session,
        Always,
        ReadOnly,
        true
    ),
    command!(
        Jobs,
        "jobs",
        [],
        "View foreground and background shell jobs",
        "",
        General,
        Always,
        ReadOnly,
        true
    ),
    command!(
        Delegate,
        "delegate",
        [],
        "Show codex-team delegation readiness or recover one durable delegation job",
        "[JOB_ID]",
        General,
        Always,
        ReadOnly,
        true
    ),
    command!(
        Evidence,
        "evidence",
        [],
        "Explain mechanistic preflight, verification, diagnosis, and cache evidence",
        "[JOB_ID|metrics]",
        General,
        Always,
        ReadOnly,
        true
    ),
    command!(
        Term,
        "term",
        ["terminal", "terminals"],
        "Manage durable tmux terminals",
        "[filter]",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Theme,
        "theme",
        ["themes"],
        "Select a built-in terminal theme",
        "[filter]",
        Display,
        IdleOnly,
        State,
        true
    ),
    command!(
        SyncTheme,
        "sync-theme",
        [],
        "Detect the current terminal background again",
        "",
        Display,
        Always,
        State,
        true
    ),
    command!(
        StatusIndicators,
        "status-indicators",
        [],
        "Configure terminal-title status indicators",
        "[on|off]",
        Display,
        Always,
        State,
        true
    ),
    command!(
        Statusline,
        "statusline",
        ["status-line"],
        "Configure statusline fields and custom status",
        "",
        Display,
        IdleOnly,
        State,
        true
    ),
    command!(
        CompactMode,
        "compact-mode",
        ["cm"],
        "Toggle compact mode (reduced padding)",
        "",
        Display,
        Always,
        State,
        true
    ),
    command!(
        LineNumbers,
        "line-numbers",
        ["ln"],
        "Toggle code line numbers",
        "",
        Display,
        Always,
        State,
        true
    ),
    command!(
        Reload,
        "reload",
        [],
        "Reload project instructions",
        "",
        Workspace,
        IdleOnly,
        State,
        true
    ),
    command!(
        Name,
        "name",
        [],
        "Name the current session",
        "<title>",
        Session,
        Always,
        State,
        true
    ),
    command!(
        Connect,
        "connect",
        [],
        "Sign in to OpenAI Codex via browser OAuth",
        "",
        General,
        IdleOnly,
        State,
        true
    ),
    command!(
        Disconnect,
        "disconnect",
        [],
        "Sign out of OpenAI Codex and revoke tokens",
        "",
        General,
        IdleOnly,
        State,
        true
    ),
];

pub fn find_command(name: &str) -> Option<&'static CommandSpec> {
    let name = name.trim().to_ascii_lowercase();
    COMMANDS
        .iter()
        .find(|spec| spec.name == name || spec.aliases.contains(&name.as_str()))
}

pub fn parse_command_line(line: &str) -> Option<CommandAction> {
    let line = line.trim().trim_start_matches('/').trim();
    let (name, args) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let spec = find_command(name)?;
    Some(CommandAction {
        id: spec.id,
        args: args.trim().to_string(),
    })
}

/// Bare token matching a known model id becomes `/model <id>` (study idea from
/// Cursor `modelSlashCommands`). Explicit commands always win.
pub fn parse_command_line_with_model_alias(
    line: &str,
    model_ids: impl IntoIterator<Item = impl AsRef<str>>,
) -> Option<CommandAction> {
    if let Some(action) = parse_command_line(line) {
        return Some(action);
    }
    let line = line.trim().trim_start_matches('/').trim();
    let (name, args) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    if name.is_empty() || !args.is_empty() {
        return None;
    }
    if !model_ids
        .into_iter()
        .any(|id| id.as_ref().eq_ignore_ascii_case(name))
    {
        return None;
    }
    Some(CommandAction {
        id: CommandId::Model,
        args: name.to_string(),
    })
}

pub fn matching_commands(query: &str, busy: bool) -> Vec<&'static CommandSpec> {
    let query = query.trim().to_ascii_lowercase();
    let mut matches = COMMANDS
        .iter()
        .filter(|spec| spec.visible)
        .filter(|spec| !busy || matches!(spec.availability, CommandAvailability::Always))
        .filter(|spec| {
            query.is_empty()
                || spec.name.contains(&query)
                || spec.aliases.iter().any(|alias| alias.contains(&query))
                || spec.description.to_ascii_lowercase().contains(&query)
        })
        .collect::<Vec<_>>();
    if query.is_empty() {
        matches.sort_by_key(|spec| {
            COMMON_DISCOVERY_COMMANDS
                .iter()
                .position(|id| *id == spec.id)
                .unwrap_or(COMMON_DISCOVERY_COMMANDS.len())
        });
    } else {
        matches.sort_by_key(|spec| command_match_rank(spec, &query));
    }
    matches
}

const COMMON_DISCOVERY_COMMANDS: &[CommandId] = &[
    CommandId::Model,
    CommandId::Resume,
    CommandId::Dashboard,
    CommandId::Permissions,
    CommandId::Diff,
    CommandId::Review,
    CommandId::Context,
    CommandId::Usage,
    CommandId::Undo,
    CommandId::New,
    CommandId::Help,
];

fn command_match_rank(spec: &CommandSpec, query: &str) -> u8 {
    if spec.name == query || spec.aliases.contains(&query) {
        0
    } else if spec.name.starts_with(query) {
        1
    } else if spec.aliases.iter().any(|alias| alias.starts_with(query)) {
        2
    } else if spec.name.contains(query) {
        3
    } else if spec.aliases.iter().any(|alias| alias.contains(query)) {
        4
    } else {
        5
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn new_session_command_is_discoverable_and_distinct_from_clear() {
        let new = find_command("new").expect("/new must be a built-in command");
        assert_ne!(new.id, CommandId::Clear);
        assert_eq!(new.availability, CommandAvailability::IdleOnly);
        assert_eq!(new.usage, "");
        assert_eq!(matching_commands("new", false)[0].name, "new");
        assert!(!matching_commands("new", true)
            .iter()
            .any(|spec| spec.name == "new"));
        assert_eq!(parse_command_line("/new").unwrap().id, new.id);
    }

    #[test]
    fn command_names_and_aliases_are_unique() {
        let mut names = HashSet::new();
        for command in COMMANDS {
            assert!(
                names.insert(command.name),
                "duplicate name: {}",
                command.name
            );
            for alias in command.aliases {
                assert!(names.insert(alias), "duplicate alias: {alias}");
            }
        }
    }

    #[test]
    fn parser_resolves_alias_and_preserves_arguments() {
        assert_eq!(
            parse_command_line("/m provider/model"),
            Some(CommandAction {
                id: CommandId::Model,
                args: "provider/model".into()
            })
        );
    }

    #[test]
    fn sessions_is_canonical_with_resume_and_continue_aliases() {
        let expected = Some(CommandAction {
            id: CommandId::Resume,
            args: "ses_example".into(),
        });
        assert_eq!(parse_command_line("/sessions ses_example"), expected);
        assert_eq!(parse_command_line("/resume ses_example"), expected);
        assert_eq!(parse_command_line("/continue ses_example"), expected);

        let sessions = find_command("sessions").unwrap();
        assert_eq!(sessions.name, "sessions");
        assert_eq!(sessions.aliases, ["resume", "continue"]);
        assert_eq!(
            sessions.description,
            "Browse and continue previous sessions"
        );

        let current = find_command("session").unwrap();
        assert_eq!(current.id, CommandId::Session);
        assert_eq!(current.availability, CommandAvailability::Always);
        assert_eq!(current.mutation, MutationClass::ReadOnly);
    }

    #[test]
    fn pane_commands_are_visible_and_available_during_streaming() {
        assert_eq!(
            parse_command_line("/split vertical ses_example"),
            Some(CommandAction {
                id: CommandId::Split,
                args: "vertical ses_example".into(),
            })
        );
        assert_eq!(
            parse_command_line("/panes next"),
            Some(CommandAction {
                id: CommandId::Pane,
                args: "next".into(),
            })
        );
        for name in ["split", "pane"] {
            let command = find_command(name).unwrap();
            assert!(command.visible);
            assert_eq!(command.availability, CommandAvailability::Always);
            assert_eq!(command.mutation, MutationClass::State);
            assert!(matching_commands(name, true)
                .iter()
                .any(|candidate| candidate.id == command.id));
        }
    }

    #[test]
    fn parser_routes_learned_recipe_cockpit_without_a_model_command() {
        assert_eq!(
            parse_command_line("/learned-recipes orchestrator cargo-check"),
            Some(CommandAction {
                id: CommandId::Recipes,
                args: "orchestrator cargo-check".into(),
            })
        );
    }

    #[test]
    fn parser_routes_self_improvement_cockpit_without_a_model_command() {
        assert_eq!(
            parse_command_line("/self-improve orchestrator"),
            Some(CommandAction {
                id: CommandId::SelfImprove,
                args: "orchestrator".into(),
            })
        );
    }

    #[test]
    fn parser_routes_mechanistic_cockpit_without_a_model_command() {
        assert_eq!(
            parse_command_line("/delegate job_123"),
            Some(CommandAction {
                id: CommandId::Delegate,
                args: "job_123".into(),
            })
        );
        let command = find_command("delegate").unwrap();
        assert_eq!(command.usage, "[JOB_ID]");
        assert_eq!(command.mutation, MutationClass::ReadOnly);
        assert_eq!(
            parse_command_line("/evidence metrics"),
            Some(CommandAction {
                id: CommandId::Evidence,
                args: "metrics".into(),
            })
        );
    }

    #[test]
    fn commit_is_visible_idle_only_workspace_command() {
        assert_eq!(
            find_command("commit").map(|spec| spec.id),
            Some(CommandId::Commit)
        );
        let spec = find_command("commit").unwrap();
        assert!(spec.visible);
        assert_eq!(spec.usage, "");
        assert_eq!(spec.availability, CommandAvailability::IdleOnly);
        assert_eq!(spec.mutation, MutationClass::Workspace);
        assert!(matching_commands("commit", false)
            .iter()
            .any(|spec| spec.id == CommandId::Commit));
        assert!(matching_commands("commit", true).is_empty());
    }

    #[test]
    fn init_is_visible_idle_only_workspace_command() {
        let spec = find_command("init").unwrap();
        assert_eq!(spec.id, CommandId::Init);
        assert!(spec.visible);
        assert_eq!(spec.usage, "");
        assert_eq!(spec.availability, CommandAvailability::IdleOnly);
        assert_eq!(spec.mutation, MutationClass::Workspace);
    }

    #[test]
    fn search_uses_aliases_descriptions_and_busy_availability() {
        assert_eq!(matching_commands("changes", false)[0].id, CommandId::Diff);
        assert_eq!(
            matching_commands("working-tree", false)[0].id,
            CommandId::Diff
        );
        assert!(matching_commands("diff", true).is_empty());
    }

    #[test]
    fn empty_discovery_prioritizes_common_commands_without_hiding_the_registry() {
        let matches = matching_commands("", false);
        let expected = [
            CommandId::Model,
            CommandId::Resume,
            CommandId::Dashboard,
            CommandId::Permissions,
            CommandId::Diff,
            CommandId::Review,
            CommandId::Context,
            CommandId::Usage,
            CommandId::Undo,
            CommandId::New,
            CommandId::Help,
        ];
        assert_eq!(
            matches
                .iter()
                .take(expected.len())
                .map(|spec| spec.id)
                .collect::<Vec<_>>(),
            expected
        );

        let visible = COMMANDS
            .iter()
            .filter(|spec| spec.visible)
            .map(|spec| spec.id)
            .collect::<HashSet<_>>();
        assert_eq!(
            matches.iter().map(|spec| spec.id).collect::<HashSet<_>>(),
            visible
        );
    }

    #[test]
    fn retry_is_visible_and_idle_only() {
        assert_eq!(
            parse_command_line("/retry"),
            Some(CommandAction {
                id: CommandId::Retry,
                args: String::new(),
            })
        );
        assert_eq!(matching_commands("retry", false)[0].id, CommandId::Retry);
        assert!(matching_commands("retry", true).is_empty());
    }

    #[test]
    fn redo_is_visible_and_idle_only() {
        assert_eq!(
            parse_command_line("/redo"),
            Some(CommandAction {
                id: CommandId::Redo,
                args: String::new(),
            })
        );
        let spec = COMMANDS
            .iter()
            .find(|candidate| candidate.id == CommandId::Redo)
            .unwrap();
        assert!(spec.visible);
        assert_eq!(spec.availability, CommandAvailability::IdleOnly);
        assert_eq!(matching_commands("redo", false)[0].id, CommandId::Redo);
        assert!(matching_commands("redo", true).is_empty());
    }

    #[test]
    fn pause_commands_are_visible_while_busy() {
        for (text, id) in [
            ("/pause", CommandId::Pause),
            ("/unpause", CommandId::Unpause),
        ] {
            assert_eq!(
                parse_command_line(text),
                Some(CommandAction {
                    id,
                    args: String::new(),
                })
            );
            let spec = COMMANDS
                .iter()
                .find(|candidate| candidate.id == id)
                .unwrap();
            assert!(spec.visible);
            assert_eq!(spec.availability, CommandAvailability::Always);
            assert_eq!(matching_commands(&text[1..], true)[0].id, id);
        }
    }

    #[test]
    fn term_command_and_aliases_are_visible_and_idle_only() {
        for (text, filter) in [
            ("/term", ""),
            ("/terminal dev", "dev"),
            ("/terminals alpha", "alpha"),
        ] {
            assert_eq!(
                parse_command_line(text),
                Some(CommandAction {
                    id: CommandId::Term,
                    args: filter.into(),
                })
            );
        }
        assert_eq!(matching_commands("term", false)[0].id, CommandId::Term);
        assert!(matching_commands("term", true)
            .iter()
            .all(|command| command.id != CommandId::Term));
    }

    #[test]
    fn theme_and_statusline_commands_are_visible_and_idle_only() {
        for (text, id, args) in [
            ("/theme amber", CommandId::Theme, "amber"),
            ("/themes", CommandId::Theme, ""),
            ("/statusline", CommandId::Statusline, ""),
            ("/status-line", CommandId::Statusline, ""),
        ] {
            assert_eq!(
                parse_command_line(text),
                Some(CommandAction {
                    id,
                    args: args.into(),
                })
            );
            let spec = COMMANDS
                .iter()
                .find(|candidate| candidate.id == id)
                .unwrap();
            assert!(spec.visible);
            assert_eq!(spec.availability, CommandAvailability::IdleOnly);
        }
        assert!(matching_commands("theme", true)
            .iter()
            .all(|command| command.id != CommandId::Theme));
        assert!(matching_commands("statusline", true)
            .iter()
            .all(|command| command.id != CommandId::Statusline));
    }

    #[test]
    fn terminal_awareness_commands_are_visible_and_available_while_busy() {
        for (text, id, args) in [
            ("/sync-theme", CommandId::SyncTheme, ""),
            ("/status-indicators", CommandId::StatusIndicators, ""),
            ("/status-indicators on", CommandId::StatusIndicators, "on"),
        ] {
            assert_eq!(
                parse_command_line(text),
                Some(CommandAction {
                    id,
                    args: args.into(),
                })
            );
            let spec = COMMANDS
                .iter()
                .find(|candidate| candidate.id == id)
                .unwrap();
            assert!(spec.visible);
            assert_eq!(spec.availability, CommandAvailability::Always);
        }
    }

    #[test]
    fn session_portability_commands_are_visible_idle_only_and_preserve_paths() {
        assert_eq!(
            parse_command_line("/export json exports/my session.json"),
            Some(CommandAction {
                id: CommandId::Export,
                args: "json exports/my session.json".into(),
            })
        );
        assert_eq!(
            parse_command_line("/import imports/my session.json"),
            Some(CommandAction {
                id: CommandId::Import,
                args: "imports/my session.json".into(),
            })
        );
        assert_eq!(
            parse_command_line("/import setup claude ./foreign setup"),
            Some(CommandAction {
                id: CommandId::Import,
                args: "setup claude ./foreign setup".into(),
            })
        );
        for id in [CommandId::Export, CommandId::Import] {
            let spec = COMMANDS
                .iter()
                .find(|candidate| candidate.id == id)
                .unwrap();
            assert!(spec.visible);
            assert_eq!(spec.availability, CommandAvailability::IdleOnly);
            assert!(!matching_commands(spec.name, true)
                .iter()
                .any(|candidate| candidate.id == id));
        }
    }

    #[test]
    fn compact_command_preserves_optional_focus_and_advertises_usage() {
        assert_eq!(
            parse_command_line("/compact focus on café authentication"),
            Some(CommandAction {
                id: CommandId::Compact,
                args: "focus on café authentication".into(),
            })
        );
        let spec = COMMANDS
            .iter()
            .find(|candidate| candidate.id == CommandId::Compact)
            .unwrap();
        assert_eq!(spec.usage, "[focus] | here [N]");
        assert_eq!(spec.availability, CommandAvailability::IdleOnly);
    }

    #[test]
    fn session_retention_is_visible_discoverable_and_idle_only() {
        assert_eq!(
            parse_command_line("/retention"),
            Some(CommandAction {
                id: CommandId::Retention,
                args: String::new(),
            })
        );
        let spec = COMMANDS
            .iter()
            .find(|candidate| candidate.id == CommandId::Retention)
            .unwrap();
        assert!(spec.visible);
        assert_eq!(spec.availability, CommandAvailability::IdleOnly);
        assert!(matching_commands("saved-session", false)
            .iter()
            .any(|candidate| candidate.id == CommandId::Retention));
        assert!(!matching_commands("retention", true)
            .iter()
            .any(|candidate| candidate.id == CommandId::Retention));
    }

    #[test]
    fn fresh_context_review_is_visible_and_idle_only() {
        let spec = COMMANDS
            .iter()
            .find(|candidate| candidate.id == CommandId::Review)
            .unwrap();
        assert!(spec.visible);
        assert_eq!(spec.availability, CommandAvailability::IdleOnly);
        assert_eq!(
            parse_command_line("review base main").unwrap().args,
            "base main"
        );
        assert!(matching_commands("fresh-context", false)
            .iter()
            .any(|candidate| candidate.id == CommandId::Review));
        assert!(!matching_commands("review", true)
            .iter()
            .any(|candidate| candidate.id == CommandId::Review));
    }

    #[test]
    fn change_walkthrough_is_visible_idle_only_and_preserves_target() {
        assert_eq!(
            parse_command_line("/walkthrough base main"),
            Some(CommandAction {
                id: CommandId::Walkthrough,
                args: "base main".into(),
            })
        );
        assert_eq!(
            parse_command_line("/explain-changes commit abcdef1")
                .expect("alias should resolve")
                .id,
            CommandId::Walkthrough
        );
        let spec = COMMANDS
            .iter()
            .find(|candidate| candidate.id == CommandId::Walkthrough)
            .unwrap();
        assert!(spec.visible);
        assert_eq!(spec.availability, CommandAvailability::IdleOnly);
        assert!(!matching_commands("walkthrough", true)
            .iter()
            .any(|candidate| candidate.id == CommandId::Walkthrough));
    }

    #[test]
    fn reasoning_and_thinking_commands_are_typed_and_discoverable() {
        assert_eq!(
            parse_command_line("/reasoning high"),
            Some(CommandAction {
                id: CommandId::Reasoning,
                args: "high".into(),
            })
        );
        assert_eq!(
            matching_commands("expanded reasoning", false)[0].id,
            CommandId::Thinking
        );
        assert_eq!(
            find_command("reasoning").unwrap().usage,
            "[none|minimal|low|medium|high|xhigh|max]"
        );
    }

    #[test]
    fn provider_command_and_plural_alias_are_visible_and_idle_only() {
        assert_eq!(
            parse_command_line("/provider mimo"),
            Some(CommandAction {
                id: CommandId::Provider,
                args: "mimo".into(),
            })
        );
        assert_eq!(
            parse_command_line("/providers deepseek"),
            Some(CommandAction {
                id: CommandId::Provider,
                args: "deepseek".into(),
            })
        );
        assert_eq!(
            parse_command_line("/provider setup mimo"),
            Some(CommandAction {
                id: CommandId::Provider,
                args: "setup mimo".into(),
            })
        );
        assert_eq!(
            parse_command_line("/providers setup"),
            Some(CommandAction {
                id: CommandId::Provider,
                args: "setup".into(),
            })
        );
        assert_eq!(
            parse_command_line("/provider discover"),
            Some(CommandAction {
                id: CommandId::Provider,
                args: "discover".into(),
            })
        );
        assert_eq!(
            parse_command_line("/providers discover"),
            Some(CommandAction {
                id: CommandId::Provider,
                args: "discover".into(),
            })
        );
        assert_eq!(
            parse_command_line("/provider templates anthropic"),
            Some(CommandAction {
                id: CommandId::Provider,
                args: "templates anthropic".into(),
            })
        );
        let provider = find_command("provider").unwrap();
        assert!(provider.visible);
        assert_eq!(provider.availability, CommandAvailability::IdleOnly);
        assert!(provider.usage.contains("templates"));
        assert!(matching_commands("templates", false)
            .iter()
            .any(|command| command.id == CommandId::Provider));
        assert!(matching_commands("provider", false)
            .iter()
            .any(|command| command.id == CommandId::Provider));
        assert!(!matching_commands("provider", true)
            .iter()
            .any(|command| command.id == CommandId::Provider));
    }

    #[test]
    fn plugin_cockpit_is_visible_idle_only_and_preserves_actions() {
        assert_eq!(
            parse_command_line("/plugins search review"),
            Some(CommandAction {
                id: CommandId::Plugin,
                args: "search review".into(),
            })
        );
        let command = find_command("plugin").unwrap();
        assert_eq!(command.availability, CommandAvailability::IdleOnly);
        assert_eq!(command.mutation, MutationClass::State);
    }

    #[test]
    fn runtime_logs_are_visible_read_only_while_render_controls_stay_hidden() {
        assert_eq!(
            parse_command_line("/logs level=warn component=render"),
            Some(CommandAction {
                id: CommandId::Logs,
                args: "level=warn component=render".into(),
            })
        );
        let logs = find_command("logs").unwrap();
        assert!(logs.visible);
        assert_eq!(logs.availability, CommandAvailability::Always);
        assert_eq!(logs.mutation, MutationClass::ReadOnly);
        let debug = find_command("debug").unwrap();
        assert!(debug.visible);
        assert!(debug.usage.is_empty());
        assert!(!matching_commands("render", false)
            .iter()
            .any(|command| command.id == CommandId::Debug));
    }

    #[test]
    fn direnv_is_visible_idle_only_and_preserves_digest_confirmation() {
        let digest = "a".repeat(64);
        assert_eq!(
            parse_command_line(&format!("/direnv confirm {digest}")),
            Some(CommandAction {
                id: CommandId::Direnv,
                args: format!("confirm {digest}"),
            })
        );
        let command = find_command("direnv").unwrap();
        assert_eq!(command.availability, CommandAvailability::IdleOnly);
        assert_eq!(command.mutation, MutationClass::State);
    }

    #[test]
    fn multi_run_and_best_of_are_visible_idle_only_and_preserve_the_request() {
        for line in ["/multi-run 3 fix the parser", "/best-of 3 fix the parser"] {
            assert_eq!(
                parse_command_line(line),
                Some(CommandAction {
                    id: CommandId::MultiRun,
                    args: "3 fix the parser".into(),
                })
            );
        }
        let command = find_command("multi-run").unwrap();
        assert!(command.visible);
        assert_eq!(command.availability, CommandAvailability::IdleOnly);
        assert!(matching_commands("isolated candidate", false)
            .iter()
            .any(|candidate| candidate.id == CommandId::MultiRun));
        assert!(!matching_commands("multi-run", true)
            .iter()
            .any(|candidate| candidate.id == CommandId::MultiRun));
    }

    #[test]
    fn mcp_oauth_actions_are_visible_idle_only_and_preserve_server_ids() {
        for (line, args) in [
            ("/mcp connect docs", "connect docs"),
            ("/mcp refresh docs", "refresh docs"),
            ("/mcp disconnect docs", "disconnect docs"),
        ] {
            assert_eq!(
                parse_command_line(line),
                Some(CommandAction {
                    id: CommandId::Mcp,
                    args: args.into(),
                })
            );
        }
        let command = find_command("mcp").unwrap();
        assert!(command.visible);
        assert_eq!(command.availability, CommandAvailability::IdleOnly);
        assert!(matching_commands("authorize", false)
            .iter()
            .any(|candidate| candidate.id == CommandId::Mcp));
        assert!(!matching_commands("mcp", true)
            .iter()
            .any(|candidate| candidate.id == CommandId::Mcp));
    }

    #[test]
    fn doctor_command_is_visible_read_only_and_idle_only() {
        assert_eq!(
            parse_command_line("/doctor"),
            Some(CommandAction {
                id: CommandId::Doctor,
                args: String::new(),
            })
        );
        let doctor = find_command("doctor").unwrap();
        assert!(doctor.visible);
        assert_eq!(doctor.availability, CommandAvailability::IdleOnly);
        assert_eq!(doctor.mutation, MutationClass::ReadOnly);
        assert!(matching_commands("readiness", false)
            .iter()
            .any(|command| command.id == CommandId::Doctor));
        assert!(matching_commands("doctor", true).is_empty());
    }

    #[test]
    fn audit_command_is_visible_read_only_and_idle_only() {
        assert_eq!(
            parse_command_line("/audit"),
            Some(CommandAction {
                id: CommandId::Audit,
                args: String::new(),
            })
        );
        let audit = find_command("audit").unwrap();
        assert!(audit.visible);
        assert_eq!(audit.availability, CommandAvailability::IdleOnly);
        assert_eq!(audit.mutation, MutationClass::ReadOnly);
        assert!(matching_commands("security", false)
            .iter()
            .any(|command| command.id == CommandId::Audit));
        assert!(!matching_commands("audit", true)
            .iter()
            .any(|command| command.id == CommandId::Audit));
    }

    #[test]
    fn raw_command_is_visible_read_only_and_idle_only() {
        assert_eq!(
            parse_command_line("/raw"),
            Some(CommandAction {
                id: CommandId::Raw,
                args: String::new(),
            })
        );
        let raw = find_command("raw").unwrap();
        assert!(raw.visible);
        assert_eq!(raw.availability, CommandAvailability::IdleOnly);
        assert_eq!(raw.mutation, MutationClass::ReadOnly);
        assert!(matching_commands("scrollback", false)
            .iter()
            .any(|command| command.id == CommandId::Raw));
        assert!(matching_commands("raw", true).is_empty());
    }

    #[test]
    fn config_hub_command_accepts_filter_and_is_idle_only() {
        assert_eq!(
            parse_command_line("/config provider"),
            Some(CommandAction {
                id: CommandId::Config,
                args: "provider".into(),
            })
        );
        let config = find_command("config").unwrap();
        assert!(config.visible);
        assert_eq!(config.availability, CommandAvailability::IdleOnly);
        assert!(matching_commands("settings", false)
            .iter()
            .any(|command| command.id == CommandId::Config));
        assert!(!matching_commands("config", true)
            .iter()
            .any(|command| command.id == CommandId::Config));
    }

    #[test]
    fn personality_questionnaire_is_discoverable_and_stays_local_while_idle() {
        for name in ["personality", "questionnaire"] {
            assert_eq!(
                parse_command_line(&format!("/{name}")),
                Some(CommandAction {
                    id: CommandId::Personality,
                    args: String::new(),
                })
            );
        }
        let personality = find_command("personality").unwrap();
        assert!(personality.visible);
        assert_eq!(personality.availability, CommandAvailability::IdleOnly);
        assert_eq!(personality.mutation, MutationClass::State);
        assert!(matching_commands("personality", false)
            .iter()
            .any(|command| command.id == CommandId::Personality));
        assert!(!matching_commands("personality", true)
            .iter()
            .any(|command| command.id == CommandId::Personality));
    }

    #[test]
    fn sandbox_command_is_visible_stateful_and_idle_only() {
        assert_eq!(
            parse_command_line("/sandbox"),
            Some(CommandAction {
                id: CommandId::Sandbox,
                args: String::new(),
            })
        );
        let sandbox = find_command("sandbox").unwrap();
        assert!(sandbox.visible);
        assert_eq!(sandbox.availability, CommandAvailability::IdleOnly);
        assert_eq!(sandbox.mutation, MutationClass::State);
        assert!(matching_commands("process isolation", false)
            .iter()
            .any(|command| command.id == CommandId::Sandbox));
        assert!(matching_commands("sandbox", true).is_empty());
    }

    #[test]
    fn context_limit_command_accepts_exact_target_and_is_idle_only() {
        assert_eq!(
            parse_command_line("/context-limit mimo/mimo-v2.5-pro"),
            Some(CommandAction {
                id: CommandId::ContextLimit,
                args: "mimo/mimo-v2.5-pro".into(),
            })
        );
        let command = find_command("context-limit").unwrap();
        assert!(command.visible);
        assert_eq!(command.availability, CommandAvailability::IdleOnly);
        assert_eq!(command.mutation, MutationClass::State);
        assert!(matching_commands("checkpoint threshold", false)
            .iter()
            .any(|command| command.id == CommandId::ContextLimit));
        assert!(matching_commands("context-limit", true).is_empty());
    }

    #[test]
    fn prompt_cache_command_accepts_exact_target_and_alias() {
        for command in [
            "/prompt-cache mimo/mimo-v2.5-pro",
            "/cache mimo/mimo-v2.5-pro",
        ] {
            assert_eq!(
                parse_command_line(command),
                Some(CommandAction {
                    id: CommandId::PromptCache,
                    args: "mimo/mimo-v2.5-pro".into(),
                })
            );
        }
        let command = find_command("prompt-cache").unwrap();
        assert!(command.visible);
        assert_eq!(command.availability, CommandAvailability::IdleOnly);
        assert_eq!(command.mutation, MutationClass::State);
        assert!(matching_commands("prompt caching", false)
            .iter()
            .any(|command| command.id == CommandId::PromptCache));
        assert!(matching_commands("prompt-cache", true).is_empty());
    }

    #[test]
    fn usage_command_is_visible_read_only_and_available_while_busy() {
        assert_eq!(
            parse_command_line("/usage"),
            Some(CommandAction {
                id: CommandId::Usage,
                args: String::new(),
            })
        );
        let command = find_command("usage").unwrap();
        assert!(command.visible);
        assert_eq!(command.availability, CommandAvailability::Always);
        assert_eq!(command.mutation, MutationClass::ReadOnly);
        assert!(matching_commands("tabbed dashboard", true)
            .iter()
            .any(|command| command.id == CommandId::Usage));
    }

    #[test]
    fn dashboard_command_is_visible_read_only_and_available_while_busy() {
        assert_eq!(
            parse_command_line("/dashboard"),
            Some(CommandAction {
                id: CommandId::Dashboard,
                args: String::new(),
            })
        );
        assert_eq!(
            parse_command_line("/agents-dashboard"),
            Some(CommandAction {
                id: CommandId::Dashboard,
                args: String::new(),
            })
        );
        let command = find_command("dashboard").unwrap();
        assert!(command.visible);
        assert_eq!(command.availability, CommandAvailability::Always);
        assert_eq!(command.mutation, MutationClass::ReadOnly);
        assert!(matching_commands("agent dashboard", true)
            .iter()
            .any(|command| command.id == CommandId::Dashboard));
    }

    #[test]
    fn session_command_is_visible_argument_free_read_only_and_available_while_busy() {
        assert_eq!(
            parse_command_line("/session"),
            Some(CommandAction {
                id: CommandId::Session,
                args: String::new(),
            })
        );
        let command = find_command("session").unwrap();
        assert!(command.visible);
        assert_eq!(command.usage, "");
        assert_eq!(command.availability, CommandAvailability::Always);
        assert_eq!(command.mutation, MutationClass::ReadOnly);
        assert!(matching_commands("sitting metrics", true)
            .iter()
            .any(|command| command.id == CommandId::Session));
    }

    #[test]
    fn trace_command_is_visible_stateful_and_available_while_busy() {
        assert_eq!(
            parse_command_line("/trace open 00000000-0000-0000-0000-000000000117"),
            Some(CommandAction {
                id: CommandId::Trace,
                args: "open 00000000-0000-0000-0000-000000000117".into(),
            })
        );
        let command = find_command("trace").unwrap();
        assert!(command.visible);
        assert_eq!(command.availability, CommandAvailability::Always);
        assert_eq!(command.mutation, MutationClass::State);
        assert!(matching_commands("provider exchange", true)
            .iter()
            .any(|candidate| candidate.id == CommandId::Trace));
    }

    #[test]
    fn filter_command_accepts_on_and_stays_available_while_busy() {
        assert_eq!(
            parse_command_line("/filter"),
            Some(CommandAction {
                id: CommandId::Filter,
                args: String::new(),
            })
        );
        assert_eq!(
            parse_command_line("/filter on"),
            Some(CommandAction {
                id: CommandId::Filter,
                args: "on".into(),
            })
        );
        assert_eq!(
            parse_command_line("/filter off"),
            Some(CommandAction {
                id: CommandId::Filter,
                args: "off".into(),
            })
        );
        let command = find_command("filter").unwrap();
        assert!(command.visible);
        assert_eq!(command.usage, "[on]");
        assert_eq!(command.availability, CommandAvailability::Always);
        assert_eq!(command.mutation, MutationClass::State);
        assert!(matching_commands("provider filter", true)
            .iter()
            .any(|command| command.id == CommandId::Filter));
    }

    #[test]
    fn request_command_is_visible_read_only_and_available_while_busy() {
        assert_eq!(
            parse_command_line("/request"),
            Some(CommandAction {
                id: CommandId::Request,
                args: String::new(),
            })
        );
        let command = find_command("request").unwrap();
        assert!(command.visible);
        assert_eq!(command.availability, CommandAvailability::Always);
        assert_eq!(command.mutation, MutationClass::ReadOnly);
        assert!(matching_commands("prepared model", true)
            .iter()
            .any(|command| command.id == CommandId::Request));
    }

    #[test]
    fn btw_preserves_optional_question_and_is_available_while_busy() {
        assert_eq!(
            parse_command_line("/btw why did that fail?"),
            Some(CommandAction {
                id: CommandId::Btw,
                args: "why did that fail?".into(),
            })
        );
        let command = find_command("btw").unwrap();
        assert!(command.visible);
        assert_eq!(command.availability, CommandAvailability::Always);
        assert_eq!(command.mutation, MutationClass::State);
        assert!(matching_commands("side question", true)
            .iter()
            .any(|command| command.id == CommandId::Btw));
    }

    #[test]
    fn keymap_is_idle_only_while_history_search_remains_available_when_busy() {
        assert_eq!(
            parse_command_line("/keymap model"),
            Some(CommandAction {
                id: CommandId::Keymap,
                args: "model".into(),
            })
        );
        assert_eq!(
            parse_command_line("/history failed build"),
            Some(CommandAction {
                id: CommandId::History,
                args: "failed build".into(),
            })
        );
        let keymap = find_command("keymap").unwrap();
        assert_eq!(keymap.availability, CommandAvailability::IdleOnly);
        assert_eq!(keymap.mutation, MutationClass::State);
        assert!(!matching_commands("keymap", true)
            .iter()
            .any(|command| command.id == CommandId::Keymap));
        let history = find_command("history").unwrap();
        assert_eq!(history.availability, CommandAvailability::Always);
        assert_eq!(history.mutation, MutationClass::ReadOnly);
        assert!(matching_commands("input history", true)
            .iter()
            .any(|command| command.id == CommandId::History));
    }

    #[test]
    fn stash_is_an_explicit_always_available_state_command() {
        assert_eq!(
            parse_command_line("/stash save"),
            Some(CommandAction {
                id: CommandId::Stash,
                args: "save".into(),
            })
        );
        let stash = find_command("stash").unwrap();
        assert_eq!(stash.availability, CommandAvailability::Always);
        assert_eq!(stash.mutation, MutationClass::State);
        assert!(matching_commands("composer drafts", true)
            .iter()
            .any(|command| command.id == CommandId::Stash));
    }

    #[test]
    fn hooks_command_accepts_filter_and_is_visible_idle_only() {
        assert_eq!(
            parse_command_line("/hooks pre tool"),
            Some(CommandAction {
                id: CommandId::Hooks,
                args: "pre tool".into(),
            })
        );
        let hooks = find_command("hooks").unwrap();
        assert!(hooks.visible);
        assert_eq!(hooks.availability, CommandAvailability::IdleOnly);
        assert_eq!(hooks.mutation, MutationClass::State);
        assert!(matching_commands("lifecycle", false)
            .iter()
            .any(|command| command.id == CommandId::Hooks));
        assert!(!matching_commands("hooks", true)
            .iter()
            .any(|command| command.id == CommandId::Hooks));
    }

    #[test]
    fn permissions_command_accepts_filter_and_is_visible_idle_only() {
        assert_eq!(
            parse_command_line("/permissions run command"),
            Some(CommandAction {
                id: CommandId::Permissions,
                args: "run command".into(),
            })
        );
        assert_eq!(
            find_command("permission").map(|spec| spec.id),
            Some(CommandId::Permissions)
        );
        let permissions = find_command("permissions").unwrap();
        assert!(permissions.visible);
        assert_eq!(permissions.availability, CommandAvailability::IdleOnly);
        assert!(!matching_commands("permissions", true)
            .iter()
            .any(|command| command.id == CommandId::Permissions));
    }

    #[test]
    fn memory_command_accepts_filter_and_export_and_is_idle_only() {
        assert_eq!(
            parse_command_line("/memory concise"),
            Some(CommandAction {
                id: CommandId::Memory,
                args: "concise".into(),
            })
        );
        assert_eq!(
            parse_command_line("/memory export notes.md"),
            Some(CommandAction {
                id: CommandId::Memory,
                args: "export notes.md".into(),
            })
        );
        let memory = find_command("memory").unwrap();
        assert!(memory.visible);
        assert_eq!(memory.availability, CommandAvailability::IdleOnly);
        assert_eq!(memory.mutation, MutationClass::State);
        assert!(!matching_commands("memory", true)
            .iter()
            .any(|command| command.id == CommandId::Memory));
    }

    #[test]
    fn dream_and_distill_are_visible_idle_only_state_commands() {
        assert_eq!(
            parse_command_line("/dream status"),
            Some(CommandAction {
                id: CommandId::Dream,
                args: "status".into(),
            })
        );
        assert_eq!(
            parse_command_line("/distill undo 00000000-0000-0000-0000-000000000001"),
            Some(CommandAction {
                id: CommandId::Distill,
                args: "undo 00000000-0000-0000-0000-000000000001".into(),
            })
        );
        for id in [CommandId::Dream, CommandId::Distill] {
            let command = COMMANDS.iter().find(|command| command.id == id).unwrap();
            assert!(command.visible);
            assert_eq!(command.availability, CommandAvailability::IdleOnly);
            assert_eq!(command.mutation, MutationClass::State);
        }
    }

    #[test]
    fn learn_is_a_visible_idle_only_learning_status_command() {
        assert_eq!(
            parse_command_line("/learn review"),
            Some(CommandAction {
                id: CommandId::Learn,
                args: "review".into(),
            })
        );
        let command = COMMANDS
            .iter()
            .find(|command| command.id == CommandId::Learn)
            .unwrap();
        assert!(command.visible);
        assert_eq!(command.availability, CommandAvailability::IdleOnly);
        assert_eq!(command.mutation, MutationClass::State);
    }

    #[test]
    fn goal_command_is_visible_and_preserves_limit_arguments() {
        assert_eq!(
            parse_command_line("/goal finish safely --iterations 3"),
            Some(CommandAction {
                id: CommandId::Goal,
                args: "finish safely --iterations 3".into(),
            })
        );
        let goal = find_command("goal").unwrap();
        assert!(goal.visible);
        assert_eq!(goal.availability, CommandAvailability::Always);
        assert_eq!(goal.mutation, MutationClass::ReadOnly);
    }

    #[test]
    fn workflow_command_is_visible_while_busy_and_preserves_paths() {
        assert_eq!(
            parse_command_line("/workflow run .ca/workflows/release.toml"),
            Some(CommandAction {
                id: CommandId::Workflow,
                args: "run .ca/workflows/release.toml".into(),
            })
        );
        assert_eq!(
            find_command("workflows").map(|command| command.id),
            Some(CommandId::Workflow)
        );
        let workflow = find_command("workflow").unwrap();
        assert!(workflow.visible);
        assert_eq!(workflow.availability, CommandAvailability::Always);
        assert!(matching_commands("workflow", true)
            .iter()
            .any(|command| command.id == CommandId::Workflow));
    }

    #[test]
    fn worktree_command_is_visible_and_preserves_safe_subcommands() {
        assert_eq!(
            parse_command_line("/worktree create review main"),
            Some(CommandAction {
                id: CommandId::Worktree,
                args: "create review main".into(),
            })
        );
        assert_eq!(
            find_command("worktrees").map(|command| command.id),
            Some(CommandId::Worktree)
        );
        let command = find_command("worktree").unwrap();
        assert!(command.visible);
        assert_eq!(command.availability, CommandAvailability::Always);
        assert_eq!(command.mutation, MutationClass::Workspace);
        assert!(matching_commands("isolated Git", true)
            .iter()
            .any(|candidate| candidate.id == CommandId::Worktree));
    }

    #[test]
    fn voice_is_visible_idle_only_and_has_no_arguments() {
        assert_eq!(
            parse_command_line("/voice"),
            Some(CommandAction {
                id: CommandId::Voice,
                args: String::new(),
            })
        );
        let voice = find_command("voice").unwrap();
        assert!(voice.visible);
        assert_eq!(voice.availability, CommandAvailability::IdleOnly);
        assert_eq!(voice.mutation, MutationClass::State);
        assert!(matching_commands("editable draft", false)
            .iter()
            .any(|command| command.id == CommandId::Voice));
        assert!(!matching_commands("voice", true)
            .iter()
            .any(|command| command.id == CommandId::Voice));
    }

    #[test]
    fn route_preset_commands_are_visible_and_preserve_names() {
        assert_eq!(
            parse_command_line("/preset daily"),
            Some(CommandAction {
                id: CommandId::Preset,
                args: "daily".into(),
            })
        );
        assert_eq!(
            parse_command_line("/preset-save daily"),
            Some(CommandAction {
                id: CommandId::PresetSave,
                args: "daily".into(),
            })
        );
        for id in [CommandId::Preset, CommandId::PresetSave] {
            let command = COMMANDS.iter().find(|command| command.id == id).unwrap();
            assert!(command.visible);
            assert_eq!(command.availability, CommandAvailability::IdleOnly);
            assert_eq!(command.mutation, MutationClass::State);
        }
    }

    #[test]
    fn every_visible_command_has_nonempty_description() {
        let visible = COMMANDS
            .iter()
            .filter(|spec| spec.visible)
            .collect::<Vec<_>>();
        assert!(
            visible.len() >= 100,
            "expected full slash registry, got {}",
            visible.len()
        );
        for spec in &visible {
            assert!(
                !spec.description.trim().is_empty(),
                "visible command /{} missing description",
                spec.name
            );
        }
        // Arg-taking commands that CA ships with empty usage: only /tasks was
        // flagged; lock its usage after hygiene. Other no-arg commands may
        // legitimately leave usage empty.
        let tasks = find_command("tasks").expect("/tasks");
        assert!(
            !tasks.usage.trim().is_empty(),
            "visible /tasks must document usage"
        );
    }

    #[test]
    fn model_alias_becomes_model_command_and_commands_win() {
        let models = ["mimo-v2.5-pro", "deepseek-v4-pro"];
        let alias = parse_command_line_with_model_alias("/mimo-v2.5-pro", models).unwrap();
        assert_eq!(alias.id, CommandId::Model);
        assert_eq!(alias.args, "mimo-v2.5-pro");
        // Explicit command wins over a coincidental model-like name.
        let cmd = parse_command_line_with_model_alias("/model mimo-v2.5-pro", models).unwrap();
        assert_eq!(cmd.id, CommandId::Model);
        assert_eq!(cmd.args, "mimo-v2.5-pro");
        // Unknown bare token stays unresolved.
        assert!(parse_command_line_with_model_alias("/not-a-model", models).is_none());
        // Alias with args is not treated as a model id.
        assert!(parse_command_line_with_model_alias("/mimo-v2.5-pro extra", models).is_none());
        // Short command /m is not overridden by model alias.
        let m = parse_command_line_with_model_alias("/m provider/x", models).unwrap();
        assert_eq!(m.id, CommandId::Model);
    }
}
