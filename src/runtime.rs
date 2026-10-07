use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ActiveProfile {
    #[default]
    Default,
    Named(String),
    Bare,
}

impl ActiveProfile {
    pub fn parse(value: Option<&str>) -> Result<Self, ProfileError> {
        let Some(value) = value else {
            return Ok(Self::Default);
        };
        if value == "default" {
            return Ok(Self::Default);
        }
        if value == "bare" {
            return Ok(Self::Bare);
        }
        let valid = !value.is_empty()
            && value.len() <= 64
            && value.as_bytes()[0].is_ascii_lowercase()
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            });
        if !valid {
            return Err(ProfileError::InvalidName);
        }
        Ok(Self::Named(value.to_string()))
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Default => "default",
            Self::Named(name) => name,
            Self::Bare => "bare",
        }
    }

    pub fn secret_namespace(&self) -> Option<&str> {
        match self {
            Self::Named(name) => Some(name),
            Self::Default | Self::Bare => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunCapabilities {
    pub mcp: bool,
    pub lsp: bool,
    pub skills: bool,
    pub plugins: bool,
    pub hooks: bool,
    pub session_persistence: bool,
    pub config_writes: bool,
    pub memory: bool,
    pub workflows: bool,
    pub worktrees: bool,
    pub agent_graphs: bool,
    pub subagents: bool,
    pub multi_run: bool,
    pub jobs: bool,
    pub email: bool,
    pub browser: bool,
    pub external_communication: bool,
    pub network: bool,
    pub learning: bool,
}

impl RunCapabilities {
    pub fn for_profile(profile: &ActiveProfile) -> Self {
        Self::for_run(profile, false)
    }

    pub fn for_run(profile: &ActiveProfile, no_session: bool) -> Self {
        let enabled = !matches!(profile, ActiveProfile::Bare);
        Self {
            mcp: enabled,
            lsp: enabled,
            skills: enabled,
            plugins: enabled,
            hooks: enabled,
            session_persistence: enabled && !no_session,
            config_writes: enabled,
            memory: enabled,
            workflows: enabled,
            worktrees: enabled,
            agent_graphs: enabled,
            subagents: enabled,
            multi_run: enabled,
            jobs: enabled,
            email: enabled,
            browser: enabled,
            external_communication: enabled,
            network: enabled,
            learning: enabled && !no_session,
        }
    }

    pub fn scheduled() -> Self {
        Self {
            mcp: false,
            lsp: false,
            skills: false,
            plugins: false,
            hooks: false,
            session_persistence: false,
            config_writes: false,
            memory: false,
            workflows: false,
            worktrees: false,
            agent_graphs: false,
            subagents: false,
            multi_run: false,
            jobs: false,
            email: false,
            browser: false,
            external_communication: false,
            network: false,
            learning: false,
        }
    }

    /// Closed capability set for one public `role-run` request.
    ///
    /// Role execution receives positive authority only from its explicitly
    /// validated tool set and process grants. Optional extensions, persistence,
    /// external communication, and model-visible network access remain absent.
    pub fn role_run() -> Self {
        Self {
            mcp: false,
            lsp: false,
            skills: false,
            plugins: false,
            hooks: false,
            session_persistence: false,
            config_writes: false,
            memory: false,
            workflows: false,
            worktrees: false,
            agent_graphs: false,
            subagents: false,
            multi_run: false,
            jobs: false,
            email: false,
            browser: false,
            external_communication: false,
            network: false,
            learning: false,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProfileError {
    #[error("profile names must be 1-64 lowercase ASCII letters, digits, '-' or '_'; 'default' and 'bare' are reserved")]
    InvalidName,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePaths {
    pub home: PathBuf,
    pub config_root: PathBuf,
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub archived_sessions_dir: PathBuf,
    pub shell_history_file: PathBuf,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RuntimePathError {
    #[error("HOME is not set; ca cannot determine runtime paths")]
    MissingHome,
}

impl RuntimePaths {
    pub fn discover() -> Result<Self, RuntimePathError> {
        Self::from_optional_home(std::env::var_os("HOME").map(PathBuf::from))
    }

    pub fn from_optional_home(home: Option<PathBuf>) -> Result<Self, RuntimePathError> {
        let home = home
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or(RuntimePathError::MissingHome)?;
        Ok(Self::from_home(home))
    }

    pub fn from_home(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        let config_root = home.join(".config/ca");
        let data_dir = home.join(".local/share/ca");
        Self {
            config_file: config_root.join("config.toml"),
            sessions_dir: data_dir.join("sessions"),
            archived_sessions_dir: data_dir.join("archive"),
            shell_history_file: config_root.join("shell_history"),
            home,
            config_root,
            data_dir,
        }
    }

    pub fn for_profile(mut self, profile: &ActiveProfile) -> Self {
        match profile {
            ActiveProfile::Default | ActiveProfile::Bare => self,
            ActiveProfile::Named(name) => {
                self.config_file = self
                    .config_root
                    .join("profiles")
                    .join(format!("{name}.toml"));
                self.sessions_dir = self.data_dir.join("profiles").join(name).join("sessions");
                self.archived_sessions_dir =
                    self.data_dir.join("profiles").join(name).join("archive");
                self
            }
        }
    }

    pub fn global_instruction_files(&self) -> [PathBuf; 2] {
        [
            self.config_root.join("AGENTS.md"),
            self.home.join(".pi/agent/AGENTS.md"),
        ]
    }

    pub fn user_skills_dir(&self) -> PathBuf {
        self.config_root.join("skills")
    }

    pub fn agent_user_skills_dir(&self) -> PathBuf {
        self.home.join(".agents/skills")
    }

    pub fn managed_skills_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("skills/installed")
    }

    pub fn skill_state_file(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("skills/state.json")
    }

    pub fn workspace_skill_config_file(
        &self,
        profile: &ActiveProfile,
        canonical_workspace: &std::path::Path,
    ) -> PathBuf {
        let digest = Sha256::digest(canonical_workspace.as_os_str().as_encoded_bytes());
        let key = digest[..16]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        self.profile_data_dir(profile)
            .join("skills/workspaces")
            .join(format!("{key}.json"))
    }

    pub fn skill_bundles_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("skill-bundles")
    }

    pub fn learning_ledger_file(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("learning/journey.json")
    }

    pub fn auto_distilled_skills_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("skills/auto-distill")
    }

    pub fn auto_distilled_commands_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("commands/auto-distill")
    }

    pub fn auto_distilled_agents_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("agents/auto-distill")
    }

    pub fn learning_state_file(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("learning/state.json")
    }

    pub fn mechanistic_quality_file(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile)
            .join("mechanistic/quality-v1.jsonl")
    }

    pub fn codex_learning_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile)
            .join("learning/codex-archive-v1")
    }

    pub fn codex_learning_state_file(&self, profile: &ActiveProfile) -> PathBuf {
        self.codex_learning_dir(profile).join("state.json")
    }

    pub fn codex_learning_lock_file(&self, profile: &ActiveProfile) -> PathBuf {
        self.codex_learning_dir(profile).join("sync.lock")
    }

    pub fn profile_data_dir(&self, profile: &ActiveProfile) -> PathBuf {
        match profile {
            ActiveProfile::Default | ActiveProfile::Bare => self.data_dir.clone(),
            ActiveProfile::Named(name) => self.data_dir.join("profiles").join(name),
        }
    }

    pub fn provider_trace_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("traces")
    }

    pub fn runtime_log_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("runtime-logs")
    }

    pub fn acceptance_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("acceptance/v1")
    }

    pub fn attachment_frecency_file(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile)
            .join("attachments/frecency.json")
    }

    pub fn user_memory_file(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("memory/user.json")
    }

    pub fn workspace_memory_file(
        &self,
        profile: &ActiveProfile,
        canonical_workspace: &std::path::Path,
    ) -> PathBuf {
        let digest = Sha256::digest(canonical_workspace.as_os_str().as_encoded_bytes());
        let key = digest[..16]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        self.profile_data_dir(profile)
            .join("memory/workspaces")
            .join(format!("{key}.json"))
    }

    pub fn worktrees_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("worktrees")
    }

    pub fn workspace_catalog_file(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile)
            .join("workspaces/catalog-v1.json")
    }

    pub fn schedules_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("schedules")
    }

    pub fn schedule_runs_v2_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.schedules_dir(profile).join("schedule-runs-v2")
    }

    pub fn schedule_private_root(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("schedule-private-v2")
    }

    pub fn email_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("email")
    }

    pub fn email_gateway_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.email_dir(profile).join("gateway-v1")
    }

    pub fn plugins_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("plugins")
    }

    pub fn plugin_catalog_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("plugin-catalog")
    }

    pub fn external_runtimes_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("external-runtimes")
    }

    pub fn local_models_dir(&self, profile: &ActiveProfile) -> PathBuf {
        self.profile_data_dir(profile).join("local-models")
    }

    pub fn self_update_dir(&self) -> PathBuf {
        self.data_dir.join("self-update")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_every_runtime_location_from_injected_home() {
        let paths = RuntimePaths::from_home("/tmp/ca-home");

        assert_eq!(
            paths.config_file,
            PathBuf::from("/tmp/ca-home/.config/ca/config.toml")
        );
        assert_eq!(
            paths.data_dir,
            PathBuf::from("/tmp/ca-home/.local/share/ca")
        );
        assert_eq!(paths.sessions_dir, paths.data_dir.join("sessions"));
        assert_eq!(paths.archived_sessions_dir, paths.data_dir.join("archive"));
        assert_eq!(
            paths.shell_history_file,
            PathBuf::from("/tmp/ca-home/.config/ca/shell_history")
        );
        assert_eq!(
            paths.user_skills_dir(),
            PathBuf::from("/tmp/ca-home/.config/ca/skills")
        );
        assert_eq!(
            paths.agent_user_skills_dir(),
            PathBuf::from("/tmp/ca-home/.agents/skills")
        );
        assert_eq!(
            paths.managed_skills_dir(&ActiveProfile::Default),
            PathBuf::from("/tmp/ca-home/.local/share/ca/skills/installed")
        );
        assert_eq!(
            paths.managed_skills_dir(&ActiveProfile::Named("work".into())),
            PathBuf::from("/tmp/ca-home/.local/share/ca/profiles/work/skills/installed")
        );
        assert_eq!(
            paths.auto_distilled_skills_dir(&ActiveProfile::Named("work".into())),
            PathBuf::from("/tmp/ca-home/.local/share/ca/profiles/work/skills/auto-distill")
        );
        assert_eq!(
            paths.learning_state_file(&ActiveProfile::Named("work".into())),
            PathBuf::from("/tmp/ca-home/.local/share/ca/profiles/work/learning/state.json")
        );
        assert_eq!(
            paths.plugin_catalog_dir(&ActiveProfile::Default),
            PathBuf::from("/tmp/ca-home/.local/share/ca/plugin-catalog")
        );
        assert_eq!(
            paths.plugin_catalog_dir(&ActiveProfile::Named("work".into())),
            PathBuf::from("/tmp/ca-home/.local/share/ca/profiles/work/plugin-catalog")
        );
        assert_eq!(
            paths.runtime_log_dir(&ActiveProfile::Default),
            PathBuf::from("/tmp/ca-home/.local/share/ca/runtime-logs")
        );
        assert_eq!(
            paths.runtime_log_dir(&ActiveProfile::Named("work".into())),
            PathBuf::from("/tmp/ca-home/.local/share/ca/profiles/work/runtime-logs")
        );
        assert_eq!(
            paths.external_runtimes_dir(&ActiveProfile::Named("work".into())),
            PathBuf::from("/tmp/ca-home/.local/share/ca/profiles/work/external-runtimes")
        );
        assert_eq!(
            paths.local_models_dir(&ActiveProfile::Named("work".into())),
            PathBuf::from("/tmp/ca-home/.local/share/ca/profiles/work/local-models")
        );
        assert_eq!(
            paths.self_update_dir(),
            PathBuf::from("/tmp/ca-home/.local/share/ca/self-update")
        );
        assert_eq!(
            paths.workspace_catalog_file(&ActiveProfile::Default),
            PathBuf::from("/tmp/ca-home/.local/share/ca/workspaces/catalog-v1.json")
        );
        assert_eq!(
            paths.workspace_catalog_file(&ActiveProfile::Named("work".into())),
            PathBuf::from("/tmp/ca-home/.local/share/ca/profiles/work/workspaces/catalog-v1.json")
        );
    }

    #[test]
    fn missing_home_is_an_error_instead_of_a_fallback() {
        let error = RuntimePaths::from_optional_home(None).unwrap_err();
        assert_eq!(
            error.to_string(),
            "HOME is not set; ca cannot determine runtime paths"
        );
    }

    #[test]
    fn named_profiles_are_validated_and_scope_config_and_sessions() {
        for valid in ["work", "work_2", "release-candidate"] {
            assert_eq!(ActiveProfile::parse(Some(valid)).unwrap().label(), valid);
        }
        for invalid in ["", "UPPER", "../escape", "has.dot", "é", &"a".repeat(65)] {
            assert_eq!(
                ActiveProfile::parse(Some(invalid)).unwrap_err(),
                ProfileError::InvalidName
            );
        }
        assert_eq!(
            ActiveProfile::parse(Some("default")).unwrap(),
            ActiveProfile::Default
        );
        assert_eq!(
            ActiveProfile::parse(Some("bare")).unwrap(),
            ActiveProfile::Bare
        );

        let paths = RuntimePaths::from_home("/tmp/ca-home")
            .for_profile(&ActiveProfile::Named("work".into()));
        assert_eq!(
            paths.config_file,
            PathBuf::from("/tmp/ca-home/.config/ca/profiles/work.toml")
        );
        assert_eq!(
            paths.sessions_dir,
            PathBuf::from("/tmp/ca-home/.local/share/ca/profiles/work/sessions")
        );
        assert_eq!(
            paths.archived_sessions_dir,
            PathBuf::from("/tmp/ca-home/.local/share/ca/profiles/work/archive")
        );
        assert_eq!(
            paths.shell_history_file,
            PathBuf::from("/tmp/ca-home/.config/ca/shell_history")
        );
    }

    #[test]
    fn memory_paths_are_profile_and_workspace_scoped() {
        let paths = RuntimePaths::from_home("/tmp/ca-home");
        let default_user = paths.user_memory_file(&ActiveProfile::Default);
        let named_user = paths.user_memory_file(&ActiveProfile::Named("work".into()));
        assert_eq!(default_user, paths.data_dir.join("memory/user.json"));
        assert_eq!(
            named_user,
            paths.data_dir.join("profiles/work/memory/user.json")
        );
        assert_ne!(
            paths.workspace_memory_file(&ActiveProfile::Default, std::path::Path::new("/a")),
            paths.workspace_memory_file(&ActiveProfile::Default, std::path::Path::new("/b"))
        );
        assert_eq!(
            paths.user_memory_file(&ActiveProfile::Bare),
            default_user,
            "bare reads the normal profile namespace without gaining writes"
        );
    }

    #[test]
    fn provider_trace_paths_are_profile_scoped() {
        let paths = RuntimePaths::from_home("/tmp/ca-home");
        assert_eq!(
            paths.provider_trace_dir(&ActiveProfile::Default),
            paths.data_dir.join("traces")
        );
        assert_eq!(
            paths.provider_trace_dir(&ActiveProfile::Named("work".into())),
            paths.data_dir.join("profiles/work/traces")
        );
        assert_eq!(
            paths.provider_trace_dir(&ActiveProfile::Bare),
            paths.data_dir.join("traces"),
            "path derivation does not grant bare persistence authority"
        );
    }

    #[test]
    fn worktree_paths_are_profile_scoped_and_bare_is_read_only_default() {
        let paths = RuntimePaths::from_home("/tmp/ca-home");
        assert_eq!(
            paths.worktrees_dir(&ActiveProfile::Default),
            paths.data_dir.join("worktrees")
        );
        assert_eq!(
            paths.worktrees_dir(&ActiveProfile::Named("work".into())),
            paths.data_dir.join("profiles/work/worktrees")
        );
        assert_eq!(
            paths.worktrees_dir(&ActiveProfile::Bare),
            paths.worktrees_dir(&ActiveProfile::Default)
        );
        assert!(RunCapabilities::for_profile(&ActiveProfile::Default).worktrees);
        assert!(!RunCapabilities::for_profile(&ActiveProfile::Bare).worktrees);
    }

    #[test]
    fn bare_disables_workflow_execution_without_changing_normal_profiles() {
        assert!(RunCapabilities::for_profile(&ActiveProfile::Default).workflows);
        assert!(RunCapabilities::for_profile(&ActiveProfile::Named("work".into())).workflows);
        assert!(!RunCapabilities::for_profile(&ActiveProfile::Bare).workflows);
    }

    #[test]
    fn bare_capabilities_disable_extensions_persistence_and_writes() {
        let capabilities = RunCapabilities::for_profile(&ActiveProfile::Bare);
        assert!(!capabilities.mcp);
        assert!(!capabilities.skills);
        assert!(!capabilities.hooks);
        assert!(!capabilities.session_persistence);
        assert!(!capabilities.config_writes);
        assert_eq!(
            RuntimePaths::from_home("/tmp/ca-home")
                .for_profile(&ActiveProfile::Bare)
                .config_file,
            PathBuf::from("/tmp/ca-home/.config/ca/config.toml")
        );
    }

    #[test]
    fn no_session_disables_only_session_persistence_for_ordinary_profiles() {
        for profile in [ActiveProfile::Default, ActiveProfile::Named("work".into())] {
            let capabilities = RunCapabilities::for_run(&profile, true);
            assert!(capabilities.mcp);
            assert!(capabilities.skills);
            assert!(capabilities.hooks);
            assert!(!capabilities.session_persistence);
            assert!(capabilities.config_writes);
        }

        assert_eq!(
            RunCapabilities::for_run(&ActiveProfile::Bare, true),
            RunCapabilities::for_profile(&ActiveProfile::Bare)
        );
    }

    #[test]
    fn role_run_capabilities_are_closed_to_persistence_extensions_and_external_authority() {
        let capabilities = RunCapabilities::role_run();

        assert!(!capabilities.session_persistence);
        assert!(!capabilities.config_writes);
        assert!(!capabilities.memory);
        assert!(!capabilities.learning);
        assert!(!capabilities.skills);
        assert!(!capabilities.plugins);
        assert!(!capabilities.mcp);
        assert!(!capabilities.lsp);
        assert!(!capabilities.hooks);
        assert!(!capabilities.agent_graphs);
        assert!(!capabilities.subagents);
        assert!(!capabilities.workflows);
        assert!(!capabilities.worktrees);
        assert!(!capabilities.multi_run);
        assert!(!capabilities.jobs);
        assert!(!capabilities.email);
        assert!(!capabilities.browser);
        assert!(!capabilities.external_communication);
        assert!(!capabilities.network);
    }
}
