//! Configuration subsystem.
//!
//! Owns `Config`, role model settings, defaults, and serde shapes for
//! `.orchestrator/config.toml`. Configuration describes policy/knobs; it does not
//! execute commands, mutate state, or decide which AI role runs next.
//!
//! Load/save and migration helpers still live in `app`.
//!
//! Read when changing config shape or defaults. Do not read for queue parsing,
//! verification evidence, or terminal rendering.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CommitMode {
    #[default]
    Auto,
    Manual,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VerificationIsolationMode {
    #[default]
    Auto,
    Required,
}

impl CommitMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Config {
    // CLI/model execution knobs.  Role models live under `roles` so the user can
    // tune intelligence/cost per role without recompiling.
    #[serde(default = "default_codex_bin")]
    pub(crate) codex_bin: String,
    // Live output is a UX feature only; it should never be used as authority.
    // Structured logs/evidence files are the durable source of truth.
    #[serde(default = "default_true")]
    pub(crate) live_output: bool,
    #[serde(default = "default_live_output_preview_lines")]
    pub(crate) live_output_preview_lines: usize,
    #[serde(default = "default_live_output_preview_chars")]
    pub(crate) live_output_preview_chars: usize,
    #[serde(default)]
    pub(crate) live_output_verbose_success: bool,
    #[serde(default = "default_true")]
    pub(crate) live_output_color: bool,
    #[serde(default)]
    pub(crate) live_output_style: LiveOutputStyle,
    #[serde(default = "default_max_cycles")]
    pub(crate) max_cycles: usize,
    #[serde(
        default = "default_master_every_completed_tasks",
        alias = "top_every_completed_tasks"
    )]
    pub(crate) master_every_completed_tasks: u64,
    #[serde(default = "default_max_task_failures")]
    pub(crate) max_task_failures: u32,
    // Historical name was "top"; "master" is still aliased for old configs.
    #[serde(default = "default_true", alias = "auto_escalate_to_top")]
    pub(crate) auto_escalate_to_master: bool,
    #[serde(default = "default_true")]
    pub(crate) require_git: bool,
    /// Automatic commits preserve legacy behavior. Manual mode creates a
    /// fingerprinted pending intent that requires `commit-pending` approval.
    #[serde(default)]
    pub(crate) commit_mode: CommitMode,
    /// Reject any non-runtime repository dirt before recovery or execution.
    #[serde(default)]
    pub(crate) require_clean_start: bool,
    /// Legacy auto/Codex configurations remain compatible. Supervised manual
    /// and ca-backed execution always require isolation regardless of `auto`.
    #[serde(default)]
    pub(crate) verification_isolation: VerificationIsolationMode,
    #[allow(dead_code)]
    #[serde(default, skip_serializing)]
    pub(crate) intern_preflight: bool,
    #[allow(dead_code)]
    #[serde(default, skip_serializing)]
    pub(crate) intern_support: InternSupport,
    #[allow(dead_code)]
    #[serde(default = "default_true", skip_serializing)]
    pub(crate) intern_read_only_commands: bool,
    #[serde(default = "default_true")]
    pub(crate) deterministic_tasks: bool,
    // Recipe harvesting promotes repeated successful Worker/verification command
    // chains into deterministic candidates.  It is safe because every command is
    // parsed through the same typed allowlist before use.
    #[serde(default = "default_true")]
    pub(crate) automatic_recipe_harvest: bool,
    #[serde(default = "default_recipe_harvest_recent_logs")]
    pub(crate) recipe_harvest_recent_logs: usize,
    #[serde(default = "default_global_observations_dir")]
    pub(crate) global_observations_dir: String,
    #[serde(default)]
    pub(crate) planner_context: PlannerContextMode,
    #[serde(default = "default_worker_prompt_hard_max_tokens")]
    pub(crate) worker_prompt_hard_max_tokens: usize,
    #[serde(default)]
    pub(crate) role_labels: RoleLabels,
    #[serde(default)]
    pub(crate) guard: GuardConfig,
    #[serde(default)]
    pub(crate) roles: Roles,
    #[serde(default)]
    pub(crate) ollama: OllamaConfig,
    #[serde(default)]
    pub(crate) mlx_lm: MlxLmConfig,
    #[serde(default)]
    pub(crate) ca: CaConfig,
    #[serde(default)]
    pub(crate) routing: RoutingConfig,
    /// Machine-wide autonomous learning remains inert until exact-digest
    /// enrollment installs the stable supervisor. These settings describe the
    /// requested policy but never grant enrollment authority by themselves.
    #[serde(default)]
    pub(crate) self_improvement: SelfImprovementConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            codex_bin: default_codex_bin(),
            live_output: true,
            live_output_preview_lines: default_live_output_preview_lines(),
            live_output_preview_chars: default_live_output_preview_chars(),
            live_output_verbose_success: false,
            live_output_color: true,
            live_output_style: LiveOutputStyle::default(),
            max_cycles: default_max_cycles(),
            master_every_completed_tasks: default_master_every_completed_tasks(),
            max_task_failures: default_max_task_failures(),
            auto_escalate_to_master: true,
            require_git: true,
            commit_mode: CommitMode::Auto,
            require_clean_start: false,
            verification_isolation: VerificationIsolationMode::Auto,
            intern_preflight: false,
            intern_support: InternSupport::default(),
            intern_read_only_commands: true,
            deterministic_tasks: true,
            automatic_recipe_harvest: true,
            recipe_harvest_recent_logs: default_recipe_harvest_recent_logs(),
            global_observations_dir: default_global_observations_dir(),
            planner_context: PlannerContextMode::default(),
            worker_prompt_hard_max_tokens: default_worker_prompt_hard_max_tokens(),
            role_labels: RoleLabels::default(),
            guard: GuardConfig::default(),
            roles: Roles::default(),
            ollama: OllamaConfig::default(),
            mlx_lm: MlxLmConfig::default(),
            ca: CaConfig::default(),
            routing: RoutingConfig::default(),
            self_improvement: SelfImprovementConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct SelfImprovementConfig {
    #[serde(default)]
    pub(crate) enabled: bool,
    #[serde(default = "default_self_improvement_mode")]
    pub(crate) mode: String,
    #[serde(default = "default_true")]
    pub(crate) continuous: bool,
    #[serde(default = "default_self_improvement_scope")]
    pub(crate) scope: String,
    #[serde(default = "default_self_improvement_providers")]
    pub(crate) providers: Vec<String>,
    #[serde(default = "default_trace_sharing")]
    pub(crate) trace_sharing: String,
    #[serde(default = "default_trace_retention")]
    pub(crate) trace_retention: String,
    #[serde(default = "default_promotion")]
    pub(crate) promotion: String,
    #[serde(default = "default_source_changes")]
    pub(crate) source_changes: String,
    #[serde(default = "default_source_branch")]
    pub(crate) source_branch: String,
    #[serde(default = "default_deployment")]
    pub(crate) deployment: String,
    #[serde(default = "default_budget")]
    pub(crate) budget: String,
    #[serde(default)]
    pub(crate) repositories: SelfImprovementRepositories,
}

impl Default for SelfImprovementConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: default_self_improvement_mode(),
            continuous: true,
            scope: default_self_improvement_scope(),
            providers: default_self_improvement_providers(),
            trace_sharing: default_trace_sharing(),
            trace_retention: default_trace_retention(),
            promotion: default_promotion(),
            source_changes: default_source_changes(),
            source_branch: default_source_branch(),
            deployment: default_deployment(),
            budget: default_budget(),
            repositories: SelfImprovementRepositories::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct SelfImprovementRepositories {
    #[serde(default)]
    pub(crate) ca: String,
    #[serde(default)]
    pub(crate) codex_team: String,
}

fn default_self_improvement_mode() -> String {
    "autonomous".to_string()
}
fn default_self_improvement_scope() -> String {
    "machine".to_string()
}
fn default_self_improvement_providers() -> Vec<String> {
    ["codex", "deepseek", "mimo"]
        .into_iter()
        .map(str::to_string)
        .collect()
}
fn default_trace_sharing() -> String {
    "full".to_string()
}
fn default_trace_retention() -> String {
    "indefinite".to_string()
}
fn default_promotion() -> String {
    "replay_only".to_string()
}
fn default_source_changes() -> String {
    "inactive".to_string()
}
fn default_source_branch() -> String {
    "current".to_string()
}
fn default_deployment() -> String {
    "rollback_only".to_string()
}
fn default_budget() -> String {
    "unbounded".to_string()
}

fn default_codex_bin() -> String {
    "codex".to_string()
}
fn default_true() -> bool {
    true
}
fn default_live_output_preview_lines() -> usize {
    8
}
fn default_live_output_preview_chars() -> usize {
    1_200
}
fn default_max_cycles() -> usize {
    50
}
fn default_master_every_completed_tasks() -> u64 {
    0
}
fn default_max_task_failures() -> u32 {
    2
}
pub(crate) fn default_recipe_harvest_recent_logs() -> usize {
    24
}
fn default_global_observations_dir() -> String {
    "~/.codex-team/observations".to_string()
}
fn default_worker_prompt_hard_max_tokens() -> usize {
    8_000
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub(crate) enum PlannerContextMode {
    #[default]
    Compact,
    Full,
}

impl PlannerContextMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Full => "full",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub(crate) enum LiveOutputStyle {
    #[default]
    Codex,
    Boxed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub(crate) enum InternSupport {
    Off,
    FailureOnly,
    #[default]
    AutoCheap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RoleLabels {
    #[serde(default = "default_master_label", alias = "top")]
    pub(crate) master: String,
    #[serde(default = "default_planner_label")]
    pub(crate) planner: String,
    #[serde(default = "default_foreman_label", alias = "middle")]
    pub(crate) foreman: String,
    #[serde(default = "default_worker_label", alias = "bottom")]
    pub(crate) worker: String,
    #[serde(default = "default_intern_label", skip_serializing)]
    pub(crate) intern: String,
}

impl Default for RoleLabels {
    fn default() -> Self {
        Self {
            master: default_master_label(),
            planner: default_planner_label(),
            foreman: default_foreman_label(),
            worker: default_worker_label(),
            intern: default_intern_label(),
        }
    }
}

pub(crate) fn default_master_label() -> String {
    "CEO".to_string()
}
pub(crate) fn default_planner_label() -> String {
    "Project Manager".to_string()
}
pub(crate) fn default_foreman_label() -> String {
    "Senior Dev".to_string()
}
pub(crate) fn default_worker_label() -> String {
    "Worker".to_string()
}
pub(crate) fn default_intern_label() -> String {
    "Intern".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GuardConfig {
    #[serde(default = "default_true", alias = "enforce_bottom_allowed_files")]
    pub(crate) enforce_worker_allowed_files: bool,
    #[serde(default = "default_guard_ignored_paths")]
    pub(crate) ignored_paths: Vec<String>,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self {
            enforce_worker_allowed_files: true,
            ignored_paths: default_guard_ignored_paths(),
        }
    }
}

pub(crate) fn default_guard_ignored_paths() -> Vec<String> {
    [
        ".git",
        ".git/**",
        ".orchestrator",
        ".orchestrator/**",
        "target",
        "target/**",
        ".build",
        ".build/**",
        ".swiftpm",
        ".swiftpm/**",
        "node_modules",
        "node_modules/**",
        ".venv",
        ".venv/**",
        "venv",
        "venv/**",
        "__pycache__",
        "__pycache__/**",
        ".pytest_cache",
        ".pytest_cache/**",
        ".next",
        ".next/**",
        "coverage",
        "coverage/**",
        "analysis/codex-wrapper",
        "analysis/codex-wrapper/**",
        "analysis/graph",
        "analysis/graph/**",
        "analysis/graph-*",
        "analysis/graph-*/**",
        "Benchmarks/Reports*",
        "Benchmarks/Reports*/**",
        "Benchmarks/Stacks",
        "Benchmarks/Stacks/**",
        "SwiftStacker.app",
        "SwiftStacker.app/**",
        "HeliconFocus.app",
        "HeliconFocus.app/**",
        "HeliconFocus.dmg",
        "export",
        "export/**",
        "practice material",
        "practice material/**",
        "dist",
        "dist/**",
        "build",
        "build/**",
        "out",
        "out/**",
        ".DS_Store",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Roles {
    #[serde(default = "default_master_role", alias = "top")]
    pub(crate) master: RoleConfig,
    #[serde(default = "default_planner_role")]
    pub(crate) planner: RoleConfig,
    #[serde(default = "default_foreman_role", alias = "middle")]
    pub(crate) foreman: RoleConfig,
    #[serde(default = "default_worker_role", alias = "bottom")]
    pub(crate) worker: RoleConfig,
    #[allow(dead_code)]
    #[serde(default = "default_intern_role", skip_serializing)]
    pub(crate) intern: RoleConfig,
}

impl Default for Roles {
    fn default() -> Self {
        Self {
            master: default_master_role(),
            planner: default_planner_role(),
            foreman: default_foreman_role(),
            worker: default_worker_role(),
            intern: default_intern_role(),
        }
    }
}

pub(crate) fn default_master_role() -> RoleConfig {
    RoleConfig {
        provider: RoleProvider::Codex,
        model: "gpt-5.5".to_string(),
        effort: "xhigh".to_string(),
        sandbox: "read-only".to_string(),
        approval_policy: "never".to_string(),
        web_search: "cached".to_string(),
        timeout_seconds: 1800,
    }
}

pub(crate) fn default_foreman_role() -> RoleConfig {
    RoleConfig {
        provider: RoleProvider::Codex,
        model: "gpt-5.5".to_string(),
        effort: "high".to_string(),
        sandbox: "read-only".to_string(),
        approval_policy: "never".to_string(),
        web_search: "disabled".to_string(),
        timeout_seconds: 1800,
    }
}

pub(crate) fn default_planner_role() -> RoleConfig {
    RoleConfig {
        provider: RoleProvider::Codex,
        model: "gpt-5.4".to_string(),
        effort: "medium".to_string(),
        sandbox: "read-only".to_string(),
        approval_policy: "never".to_string(),
        web_search: "cached".to_string(),
        timeout_seconds: 1800,
    }
}

pub(crate) fn default_worker_role() -> RoleConfig {
    RoleConfig {
        provider: RoleProvider::Codex,
        model: "gpt-5.4-mini".to_string(),
        effort: "medium".to_string(),
        sandbox: "workspace-write".to_string(),
        approval_policy: "never".to_string(),
        web_search: "disabled".to_string(),
        timeout_seconds: 900,
    }
}

pub(crate) fn default_intern_role() -> RoleConfig {
    RoleConfig {
        provider: RoleProvider::Codex,
        model: "gpt-5.3-codex-spark".to_string(),
        effort: "low".to_string(),
        sandbox: "read-only".to_string(),
        approval_policy: "never".to_string(),
        web_search: "disabled".to_string(),
        timeout_seconds: 300,
    }
}

/// Which model process executes a role call. Default remains Codex.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RoleProvider {
    #[default]
    Codex,
    Ollama,
    /// Apple Silicon `mlx_lm.server` OpenAI-compatible endpoint.
    #[serde(alias = "mlx-lm")]
    MlxLm,
    Ca,
}

impl RoleProvider {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Ollama => "ollama",
            Self::MlxLm => "mlx_lm",
            Self::Ca => "ca",
        }
    }

    #[allow(dead_code)] // used by provider unit tests; doctor uses explicit match arms
    pub(crate) fn is_local_http(self) -> bool {
        matches!(self, Self::Ollama | Self::MlxLm)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CaConfig {
    /// Fail-closed opt-in for the external `ca role-run` execution engine.
    #[serde(default)]
    pub(crate) enabled: bool,
    /// Absolute path to the `ca` executable. Validated before any role traffic.
    #[serde(default)]
    pub(crate) bin: String,
    /// Named, nonreserved `ca` profile used for process-scoped role execution.
    #[serde(default = "default_ca_profile")]
    pub(crate) profile: String,
    /// Internal provider route configured in the selected `ca` profile.
    #[serde(default = "default_ca_route_provider")]
    pub(crate) route_provider: String,
}

impl Default for CaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bin: String::new(),
            profile: default_ca_profile(),
            route_provider: default_ca_route_provider(),
        }
    }
}

fn default_ca_profile() -> String {
    "codex-team".to_string()
}

fn default_ca_route_provider() -> String {
    "openai".to_string()
}

impl Config {
    pub(crate) fn requires_verification_isolation(&self) -> bool {
        self.verification_isolation == VerificationIsolationMode::Required
            || self.commit_mode == CommitMode::Manual
            || self.roles.any_role_uses_ca()
    }

    /// Validate opt-in `ca` configuration without inspecting credentials or
    /// mutating runtime profiles. Disabled, unused configuration remains inert.
    pub(crate) fn validate_ca(&self) -> Result<()> {
        let selected = self.roles.any_role_uses_ca();
        let worker_selected = self.roles.worker.provider == RoleProvider::Ca;
        if selected && !self.ca.enabled {
            bail!("provider = \"ca\" requires [ca].enabled = true");
        }
        if worker_selected && !self.guard.enforce_worker_allowed_files {
            bail!(
                "provider = \"ca\" for the Worker requires [guard].enforce_worker_allowed_files = true"
            );
        }
        if worker_selected {
            let built_in_ignored = default_guard_ignored_paths();
            if let Some((index, _)) = self
                .guard
                .ignored_paths
                .iter()
                .enumerate()
                .find(|(_, pattern)| !built_in_ignored.contains(pattern))
            {
                bail!(
                    "provider = \"ca\" for the Worker permits only built-in generated/runtime [guard].ignored_paths; custom pattern at index {index} could blind the authoritative post-run scope guard"
                );
            }
        }
        if !self.ca.enabled && !selected {
            return Ok(());
        }
        if matches!(self.ca.profile.trim(), "" | "default" | "bare") {
            bail!("[ca].profile must be a nonreserved named profile (not default or bare)");
        }
        if !is_safe_ca_identifier(&self.ca.profile) {
            bail!(
                "[ca].profile must contain 1-128 safe ASCII identifier characters (letters, digits, '-', '_', or '.')"
            );
        }
        if !is_safe_ca_identifier(&self.ca.route_provider) {
            bail!(
                "[ca].route_provider must contain 1-128 safe ASCII identifier characters (letters, digits, '-', '_', or '.')"
            );
        }
        let bin = Path::new(&self.ca.bin);
        if !bin.is_absolute() {
            bail!("[ca].bin must be an absolute path");
        }
        let canonical = fs::canonicalize(bin)
            .with_context(|| format!("canonicalize [ca].bin {}", bin.display()))?;
        let metadata = fs::metadata(&canonical)
            .with_context(|| format!("inspect [ca].bin {}", canonical.display()))?;
        if !metadata.is_file() {
            bail!("[ca].bin must name a regular file");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                bail!("[ca].bin must be executable");
            }
        }
        Ok(())
    }
}

fn is_safe_ca_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RoleConfig {
    /// Model backend for this role. `sandbox`, `approval_policy`, and
    /// `web_search` apply to Codex only and are ignored by `ca`. `effort` is
    /// also forwarded to `ca` as its per-run reasoning effort.
    #[serde(default)]
    pub(crate) provider: RoleProvider,
    pub(crate) model: String,
    pub(crate) effort: String,
    pub(crate) sandbox: String,
    pub(crate) approval_policy: String,
    pub(crate) web_search: String,
    pub(crate) timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OllamaConfig {
    /// When false, any role with `provider = "ollama"` fails closed (no Codex fallback).
    #[serde(default)]
    pub(crate) enabled: bool,
    /// Ollama root URL without `/v1` (chat uses `{base_url}/v1/chat/completions`).
    #[serde(default = "default_ollama_base_url")]
    pub(crate) base_url: String,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: default_ollama_base_url(),
        }
    }
}

fn default_ollama_base_url() -> String {
    "http://127.0.0.1:11434".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MlxLmConfig {
    /// When false, any role with `provider = "mlx_lm"` fails closed (no Codex fallback).
    #[serde(default)]
    pub(crate) enabled: bool,
    /// `mlx_lm.server` root URL (default port 8080). Chat uses `{base_url}/v1/chat/completions`.
    #[serde(default = "default_mlx_lm_base_url")]
    pub(crate) base_url: String,
}

impl Default for MlxLmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: default_mlx_lm_base_url(),
        }
    }
}

fn default_mlx_lm_base_url() -> String {
    "http://127.0.0.1:8080".to_string()
}

/// Soft routing knobs. Provider selection still lives on `roles.*.provider`
/// unless a future opt-in auto-apply flag is enabled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RoutingConfig {
    /// Reserved: when true, Rust may apply advisory provider hints to config.
    /// Default false — hints are observation/receipt only.
    #[serde(default)]
    pub(crate) allow_provider_override: bool,
    /// Consecutive malformed/unreachable local-provider failures before `step` stops.
    #[serde(default = "default_local_provider_failure_budget")]
    pub(crate) local_provider_failure_budget: u32,
    /// When true, honor scorecard soft gates: skip Worker when Rust lane is
    /// recommended for a deterministic candidate, or pause on Worker cooldown.
    /// Default false — scorecards remain advisory-only.
    #[serde(default)]
    pub(crate) honor_scorecard_soft_gate: bool,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            allow_provider_override: false,
            local_provider_failure_budget: default_local_provider_failure_budget(),
            honor_scorecard_soft_gate: false,
        }
    }
}

fn default_local_provider_failure_budget() -> u32 {
    3
}

impl Roles {
    pub(crate) fn any_role_uses(&self, provider: RoleProvider) -> bool {
        [
            self.master.provider,
            self.planner.provider,
            self.foreman.provider,
            self.worker.provider,
        ]
        .into_iter()
        .any(|role_provider| role_provider == provider)
    }

    pub(crate) fn any_role_uses_ollama(&self) -> bool {
        self.any_role_uses(RoleProvider::Ollama)
    }

    pub(crate) fn any_role_uses_mlx_lm(&self) -> bool {
        self.any_role_uses(RoleProvider::MlxLm)
    }

    pub(crate) fn any_role_uses_ca(&self) -> bool {
        self.any_role_uses(RoleProvider::Ca)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_config_gets_safe_integration_defaults() {
        let cfg: Config = toml::from_str("codex_bin = \"codex\"\n").unwrap();
        assert_eq!(cfg.commit_mode, CommitMode::Auto);
        assert!(!cfg.require_clean_start);
        assert!(!cfg.ca.enabled);
        assert_eq!(cfg.ca.profile, "codex-team");
        assert_eq!(cfg.ca.route_provider, "openai");
        assert!(!cfg.self_improvement.enabled);
        assert_eq!(cfg.self_improvement.mode, "autonomous");
        assert!(cfg.self_improvement.continuous);
        assert_eq!(cfg.self_improvement.scope, "machine");
        assert_eq!(
            cfg.self_improvement.providers,
            ["codex", "deepseek", "mimo"]
        );
        assert_eq!(cfg.self_improvement.promotion, "replay_only");
        assert_eq!(cfg.self_improvement.source_changes, "inactive");
        assert_eq!(cfg.self_improvement.deployment, "rollback_only");
        assert_eq!(cfg.self_improvement.budget, "unbounded");
    }

    #[test]
    fn self_improvement_config_deserializes_without_enrolling_or_mutating() {
        let cfg: Config = toml::from_str(
            r#"
[self_improvement]
enabled = true
mode = "autonomous"
continuous = true
scope = "machine"
providers = ["codex", "deepseek", "mimo"]
trace_sharing = "full"
trace_retention = "indefinite"
promotion = "replay_only"
source_changes = "roadmap_and_evidence"
source_branch = "current"
deployment = "independent_slots"
budget = "unbounded"

[self_improvement.repositories]
ca = "/tmp/ca"
codex_team = "/tmp/codex-team"
"#,
        )
        .unwrap();
        assert!(cfg.self_improvement.enabled);
        assert_eq!(cfg.self_improvement.repositories.ca, "/tmp/ca");
        assert_eq!(
            cfg.self_improvement.repositories.codex_team,
            "/tmp/codex-team"
        );
    }

    #[test]
    fn ca_role_fails_closed_when_integration_is_disabled() {
        let mut cfg = Config::default();
        cfg.roles.worker.provider = RoleProvider::Ca;
        let error = cfg.validate_ca().unwrap_err().to_string();
        assert!(error.contains("[ca].enabled = true"));
    }

    #[test]
    fn enabled_ca_rejects_reserved_profile_before_execution() {
        let mut cfg = Config::default();
        cfg.ca.enabled = true;
        cfg.ca.profile = "default".to_string();
        let error = cfg.validate_ca().unwrap_err().to_string();
        assert!(error.contains("nonreserved"));
    }

    #[test]
    fn enabled_ca_rejects_unsafe_profile_and_route_before_execution() {
        let mut cfg = Config::default();
        cfg.ca.enabled = true;
        cfg.ca.profile = "codex team".to_string();
        let error = cfg.validate_ca().unwrap_err().to_string();
        assert!(error.contains("safe ASCII"));

        cfg.ca.profile = "codex-team".to_string();
        cfg.ca.route_provider = "openai/provider".to_string();
        let error = cfg.validate_ca().unwrap_err().to_string();
        assert!(error.contains("route_provider"));
        assert!(error.contains("safe ASCII"));
    }

    #[test]
    fn ca_worker_fails_closed_when_worker_file_guard_is_disabled() {
        let mut cfg = Config::default();
        cfg.ca.enabled = true;
        cfg.roles.worker.provider = RoleProvider::Ca;
        cfg.guard.enforce_worker_allowed_files = false;

        let error = cfg.validate_ca().unwrap_err().to_string();

        assert!(error.contains("enforce_worker_allowed_files = true"));
    }

    #[test]
    fn ca_worker_rejects_custom_ignored_paths_that_can_blind_scope_guard() {
        for ignored in ["src/**", "**", "Oracles/**", "custom-cache/**"] {
            let mut cfg = Config::default();
            cfg.ca.enabled = true;
            cfg.roles.worker.provider = RoleProvider::Ca;
            cfg.guard.ignored_paths.push(ignored.to_string());

            let error = cfg.validate_ca().unwrap_err().to_string();

            assert!(error.contains("only built-in"), "{ignored}: {error}");
            assert!(error.contains("could blind"), "{ignored}: {error}");
        }
    }

    #[test]
    fn ca_worker_accepts_unmodified_builtin_guard_ignores() {
        let mut cfg = Config::default();
        cfg.ca.enabled = true;
        cfg.ca.bin = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        cfg.roles.worker.provider = RoleProvider::Ca;

        cfg.validate_ca().unwrap();
    }

    #[test]
    fn advisory_only_ca_roles_do_not_require_worker_file_guard() {
        let mut cfg = Config::default();
        cfg.ca.enabled = true;
        cfg.ca.bin = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        cfg.roles.master.provider = RoleProvider::Ca;
        cfg.guard.enforce_worker_allowed_files = false;
        cfg.guard.ignored_paths.push("src/**".to_string());

        cfg.validate_ca().unwrap();
    }
}
