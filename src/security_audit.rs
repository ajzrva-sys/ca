use crate::config::CaConfig;
use crate::runtime::{ActiveProfile, RuntimePaths};
use std::collections::{BTreeSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Take};
use std::path::{Path, PathBuf};

const MAX_SCAN_FILES: usize = 512;
const MAX_SCAN_ENTRIES: usize = 2_048;
const MAX_FILE_BYTES: usize = 64 * 1024;
const MAX_TOTAL_BYTES: usize = 2 * 1024 * 1024;
const MAX_SCAN_DEPTH: usize = 8;
const MAX_FINDINGS: usize = 128;
const MAX_REPORT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AuditSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditFinding {
    pub severity: AuditSeverity,
    pub rule_id: &'static str,
    pub path: String,
    pub evidence: String,
    pub remediation: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditReport {
    pub workspace: String,
    pub profile: String,
    pub scanned_files: usize,
    pub scanned_bytes: usize,
    pub partial: bool,
    pub findings: Vec<AuditFinding>,
}

impl AuditReport {
    pub fn has_high_or_critical(&self) -> bool {
        self.findings.iter().any(|finding| {
            matches!(
                finding.severity,
                AuditSeverity::High | AuditSeverity::Critical
            )
        })
    }

    pub fn render_text(&self) -> String {
        let mut counts = [0usize; 5];
        for finding in &self.findings {
            counts[finding.severity as usize] += 1;
        }
        let mut output = format!(
            "Agent-harness audit\n\nWorkspace: {}\nProfile: {}\nStatus: {}\nScanned: {} files · {} bytes\nFindings: {} critical · {} high · {} medium · {} low · {} info",
            safe_text(&self.workspace, 256),
            safe_text(&self.profile, 64),
            if self.partial { "PARTIAL" } else { "COMPLETE" },
            self.scanned_files,
            self.scanned_bytes,
            counts[AuditSeverity::Critical as usize],
            counts[AuditSeverity::High as usize],
            counts[AuditSeverity::Medium as usize],
            counts[AuditSeverity::Low as usize],
            counts[AuditSeverity::Info as usize],
        );
        if self.findings.is_empty() {
            output.push_str("\n\nNo findings.");
        } else {
            output.push_str("\n\nFindings");
            for finding in &self.findings {
                output.push_str(&format!(
                    "\n\n[{}] {} · {}\nEvidence: {}\nRemediation: {}",
                    finding.severity.label(),
                    finding.rule_id,
                    safe_text(&finding.path, 512),
                    safe_text(&finding.evidence, 1024),
                    finding.remediation,
                ));
            }
        }
        bound_report(output)
    }
}

impl AuditSeverity {
    fn label(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
            Self::Critical => "CRITICAL",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuditRequest {
    pub workspace_root: PathBuf,
    pub cwd: PathBuf,
    pub runtime_paths: RuntimePaths,
    pub profile: ActiveProfile,
    pub loaded_config: Option<CaConfig>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("the primary workspace could not be inspected safely")]
    WorkspaceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditCliArgs {
    pub active_profile: ActiveProfile,
}

pub fn run_cli(cli: AuditCliArgs) -> i32 {
    let runtime_paths = match RuntimePaths::discover() {
        Ok(paths) => paths,
        Err(_) => {
            eprintln!("ca audit: runtime paths are unavailable");
            return 2;
        }
    };
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => {
            eprintln!("ca audit: current directory is unavailable");
            return 2;
        }
    };
    let workspace = match crate::workspace::WorkspaceContext::discover(&cwd) {
        Ok(workspace) => workspace,
        Err(_) => {
            eprintln!("ca audit: primary workspace is unavailable");
            return 2;
        }
    };
    let Some(primary) = workspace.primary() else {
        eprintln!("ca audit: primary workspace is unavailable");
        return 2;
    };
    let report = match run(AuditRequest {
        workspace_root: primary.path.clone(),
        cwd,
        runtime_paths,
        profile: cli.active_profile,
        loaded_config: None,
    }) {
        Ok(report) => report,
        Err(_) => {
            eprintln!("ca audit: audit could not complete");
            return 2;
        }
    };
    use std::io::Write as _;
    let mut stdout = std::io::stdout().lock();
    if let Err(error) = writeln!(stdout, "{}", report.render_text()) {
        return if error.kind() == std::io::ErrorKind::BrokenPipe {
            0
        } else {
            2
        };
    }
    i32::from(report.has_high_or_critical())
}

pub fn run(request: AuditRequest) -> Result<AuditReport, AuditError> {
    let workspace =
        fs::canonicalize(&request.workspace_root).map_err(|_| AuditError::WorkspaceUnavailable)?;
    let metadata = fs::symlink_metadata(&request.workspace_root)
        .map_err(|_| AuditError::WorkspaceUnavailable)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(AuditError::WorkspaceUnavailable);
    }
    let cwd = fs::canonicalize(&request.cwd).unwrap_or_else(|_| workspace.clone());
    let profile_paths = request.runtime_paths.clone().for_profile(&request.profile);
    let mut scanner = Scanner::new(
        workspace.clone(),
        cwd,
        request.runtime_paths,
        profile_paths,
        request.profile.clone(),
    );
    scanner.scan(request.loaded_config);
    Ok(scanner.finish())
}

struct Scanner {
    workspace: PathBuf,
    cwd: PathBuf,
    base_paths: RuntimePaths,
    profile_paths: RuntimePaths,
    profile: ActiveProfile,
    scanned_files: usize,
    scanned_entries: usize,
    scanned_bytes: usize,
    partial: bool,
    findings: Vec<AuditFinding>,
    finding_keys: BTreeSet<String>,
}

impl Scanner {
    fn new(
        workspace: PathBuf,
        cwd: PathBuf,
        base_paths: RuntimePaths,
        profile_paths: RuntimePaths,
        profile: ActiveProfile,
    ) -> Self {
        Self {
            workspace,
            cwd,
            base_paths,
            profile_paths,
            profile,
            scanned_files: 0,
            scanned_entries: 0,
            scanned_bytes: 0,
            partial: false,
            findings: Vec::new(),
            finding_keys: BTreeSet::new(),
        }
    }

    fn scan(&mut self, loaded_config: Option<CaConfig>) {
        if !self.cwd.starts_with(&self.workspace) {
            self.add(
                AuditSeverity::High,
                "AUD-PATH-001",
                "workspace",
                "active directory is outside the canonical primary workspace",
                "Run the audit from inside the intended primary workspace.",
            );
            self.cwd = self.workspace.clone();
        }
        self.scan_instructions();
        let config = self.scan_config(loaded_config);
        self.scan_skills(&config);
        self.scan_configured_surfaces(&config);
    }

    fn finish(mut self) -> AuditReport {
        self.findings.sort_by(|left, right| {
            right
                .severity
                .cmp(&left.severity)
                .then_with(|| left.rule_id.cmp(right.rule_id))
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.evidence.cmp(&right.evidence))
        });
        AuditReport {
            workspace: "workspace".into(),
            profile: self.profile.label().into(),
            scanned_files: self.scanned_files,
            scanned_bytes: self.scanned_bytes,
            partial: self.partial,
            findings: self.findings,
        }
    }

    fn scan_instructions(&mut self) {
        let mut paths = BTreeSet::new();
        if !matches!(self.profile, ActiveProfile::Bare) {
            paths.extend(self.base_paths.global_instruction_files());
        }
        let mut current = Some(self.cwd.as_path());
        while let Some(directory) = current {
            paths.insert(directory.join("AGENTS.md"));
            paths.insert(directory.join("CLAUDE.md"));
            if directory == self.workspace {
                break;
            }
            current = directory.parent();
        }
        for path in paths {
            self.scan_text_file(&path, &self.authority_for(&path), "instruction");
        }

        let rules = self.workspace.join(".ca/rules");
        self.scan_tree(&rules, &self.workspace.clone(), true, "instruction rule");
    }

    fn scan_config(&mut self, loaded_config: Option<CaConfig>) -> CaConfig {
        if matches!(self.profile, ActiveProfile::Bare) {
            return CaConfig::default();
        }
        let path = self.profile_paths.config_file.clone();
        let authority = self.base_paths.config_root.clone();
        let raw = self.scan_text_file(&path, &authority, "profile configuration");
        match raw {
            Some(raw) => match toml::from_str::<CaConfig>(&raw) {
                Ok(configured) => loaded_config.unwrap_or(configured),
                Err(_) => {
                    self.add(
                        AuditSeverity::High,
                        "AUD-CONFIG-001",
                        self.label_path(&path),
                        "active profile configuration is structurally invalid",
                        "Repair the invalid configuration fields before enabling harness integrations.",
                    );
                    loaded_config.unwrap_or_default()
                }
            },
            None => loaded_config.unwrap_or_default(),
        }
    }

    fn scan_skills(&mut self, config: &CaConfig) {
        let mut roots = BTreeSet::new();
        let mut current = Some(self.cwd.as_path());
        while let Some(directory) = current {
            roots.insert(directory.join(".agents/skills"));
            if directory == self.workspace {
                break;
            }
            current = directory.parent();
        }
        if !matches!(self.profile, ActiveProfile::Bare) {
            roots.insert(self.base_paths.agent_user_skills_dir());
            roots.insert(self.base_paths.user_skills_dir());
            roots.insert(self.base_paths.home.join(".claude/skills"));
            roots.insert(self.base_paths.managed_skills_dir(&self.profile));
            roots.insert(PathBuf::from("/etc/codex/skills"));
            roots.insert(self.workspace.join(".ca/skills"));
            for configured in &config.skills.compatibility_roots {
                roots.insert(if configured.is_absolute() {
                    configured.clone()
                } else {
                    self.workspace.join(configured)
                });
            }
        }
        for root in roots {
            let allowed = self.allowed_skill_root(&root);
            if !allowed {
                self.add(
                    AuditSeverity::High,
                    "AUD-PATH-001",
                    self.label_path(&root),
                    "configured skill root is outside loaded workspace/profile authorities",
                    "Move the skill under an approved workspace, user, managed, or administrator skill root.",
                );
                continue;
            }
            self.scan_tree(&root, &root, true, "skill file");
        }
    }

    fn scan_configured_surfaces(&mut self, config: &CaConfig) {
        for (index, hook) in config.hooks.iter().enumerate() {
            let label = format!("profile/hooks/{index}");
            if crate::hooks::HookEvent::parse(&hook.event).is_none()
                || hook.command.trim().is_empty()
                || !(1..=300).contains(&hook.timeout_secs)
            {
                self.add(
                    AuditSeverity::High,
                    "AUD-DEFINITION-001",
                    label.clone(),
                    "hook definition is incomplete or invalid",
                    "Repair the hook event, command, and timeout before enabling it.",
                );
            }
            self.audit_command(&hook.command, &label, "hook");
        }

        let mut mcp_ids = config.mcp.servers.keys().collect::<Vec<_>>();
        mcp_ids.sort();
        for id in mcp_ids {
            let server = &config.mcp.servers[id];
            let label = format!("profile/mcp/{}", safe_text(id, 128));
            match server.transport {
                crate::config::McpTransport::Stdio => {
                    if server.command.trim().is_empty() {
                        self.add(
                            AuditSeverity::High,
                            "AUD-DEFINITION-001",
                            label.clone(),
                            "enabled stdio MCP definition has no executable",
                            "Configure one exact executable or disable the MCP server.",
                        );
                    } else {
                        self.audit_command(&server.command, &label, "MCP");
                    }
                }
                crate::config::McpTransport::StreamableHttp => {
                    let valid = server
                        .url
                        .as_deref()
                        .and_then(|value| url::Url::parse(value).ok());
                    match valid {
                        Some(url) if url.scheme() == "https" => self.add(
                            AuditSeverity::Medium,
                            "AUD-MCP-001",
                            label.clone(),
                            "remote MCP crosses a network trust boundary (host withheld)",
                            "Confirm the remote operator, exposed tools, authentication, and network grant.",
                        ),
                        Some(_) => self.add(
                            AuditSeverity::High,
                            "AUD-MCP-002",
                            label.clone(),
                            "remote MCP is not pinned to HTTPS",
                            "Use an authenticated HTTPS endpoint or disable the remote MCP server.",
                        ),
                        None => self.add(
                            AuditSeverity::High,
                            "AUD-DEFINITION-001",
                            label.clone(),
                            "remote MCP URL is absent or invalid",
                            "Configure one valid HTTPS URL or disable the MCP server.",
                        ),
                    }
                }
            }
        }

        let mut lsp_ids = config.lsp.servers.keys().collect::<Vec<_>>();
        lsp_ids.sort();
        for id in lsp_ids {
            let server = &config.lsp.servers[id];
            if server.enabled {
                self.audit_command(
                    &server.command,
                    &format!("profile/lsp/{}", safe_text(id, 128)),
                    "language server",
                );
            }
        }
        if let Some(command) = config.ui.statusline.custom_command.as_deref() {
            self.audit_command(command, "profile/ui/statusline", "status worker");
        }
        if let Some(executable) = config.email.executable.as_ref() {
            self.audit_command(
                &executable.to_string_lossy(),
                "profile/email",
                "email helper",
            );
        }
        if let Some(crate::config::VoiceBackendConfig::Local { executable, .. }) =
            config.voice.backend.as_ref()
        {
            self.audit_command(
                &executable.to_string_lossy(),
                "profile/voice",
                "voice helper",
            );
        }

        for (index, rule) in config.permission_rules.iter().enumerate() {
            if let Err(_error) = rule.validate() {
                self.add(
                    AuditSeverity::High,
                    "AUD-DEFINITION-001",
                    format!("profile/permissions/{index}"),
                    "permission definition is invalid",
                    "Replace it with a bounded valid tool, path, or command-prefix matcher.",
                );
                continue;
            }
            if rule.decision == crate::permission::PermissionDecision::Allow
                && permission_is_broad(&rule.matcher)
            {
                self.add(
                    AuditSeverity::High,
                    "AUD-PERM-001",
                    format!("profile/permissions/{index}"),
                    "allow rule grants a broad mutation or command surface",
                    "Narrow the rule to an exact tool resource, path, or multi-token command prefix.",
                );
            }
        }
        for (index, rule) in config.approval_rules.iter().enumerate() {
            if legacy_permission_is_broad(rule) {
                self.add(
                    AuditSeverity::High,
                    "AUD-PERM-001",
                    format!("profile/legacy_permissions/{index}"),
                    "legacy allow rule grants a broad mutation or command surface",
                    "Replace the legacy rule with a narrow typed permission rule.",
                );
            }
        }
    }

    fn audit_command(&mut self, command: &str, label: &str, kind: &str) {
        let command = command.trim();
        if command.is_empty() {
            return;
        }
        if contains_shell_composition(command) {
            self.add(
                AuditSeverity::High,
                "AUD-EXEC-001",
                label.to_string(),
                format!("{kind} command uses shell composition or substitution"),
                "Use one exact executable and argument vector without shell composition.",
            );
            return;
        }
        let Some(token) = first_command_token(command) else {
            self.add(
                AuditSeverity::High,
                "AUD-EXEC-002",
                label.to_string(),
                format!("{kind} executable could not be resolved safely"),
                "Configure an exact bounded executable path.",
            );
            return;
        };
        let resolved = if token.contains('/') {
            let candidate = PathBuf::from(&token);
            Some(if candidate.is_absolute() {
                candidate
            } else {
                self.workspace.join(candidate)
            })
        } else {
            resolve_in_path(&token)
        };
        let Some(path) = resolved else {
            self.add(
                AuditSeverity::High,
                "AUD-EXEC-002",
                label.to_string(),
                format!("{kind} executable is not resolvable without running it"),
                "Configure an existing trusted executable using an absolute path.",
            );
            return;
        };
        self.inspect_executable(&path, label, kind);
    }

    fn inspect_executable(&mut self, path: &Path, label: &str, kind: &str) {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(_) => {
                self.add(
                    AuditSeverity::High,
                    "AUD-EXEC-002",
                    label.to_string(),
                    format!("{kind} executable is missing or unreadable"),
                    "Install or configure an existing trusted executable explicitly.",
                );
                return;
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            self.add(
                AuditSeverity::High,
                "AUD-EXEC-003",
                label.to_string(),
                format!("{kind} executable is a symlink or non-regular file"),
                "Pin the integration to a trusted regular executable.",
            );
            return;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.mode() & 0o111 == 0 {
                self.add(
                    AuditSeverity::High,
                    "AUD-EXEC-003",
                    label.to_string(),
                    format!("{kind} target is not executable"),
                    "Configure a trusted executable file.",
                );
            }
            if metadata.mode() & 0o022 != 0 {
                self.add(
                    AuditSeverity::Critical,
                    "AUD-PERMISSIONS-001",
                    label.to_string(),
                    format!("{kind} executable is writable by group or others"),
                    "Remove group/other write permission or choose a protected executable.",
                );
            }
            self.audit_owner(metadata.uid(), label, "executable");
        }
    }

    fn scan_tree(&mut self, root: &Path, authority: &Path, text: bool, kind: &str) {
        let root_metadata = match fs::symlink_metadata(root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) => {
                self.mark_partial(
                    self.label_path(root),
                    format!("{kind} root could not be inspected"),
                );
                return;
            }
        };
        if !self.inspect_metadata(root, &root_metadata, authority, kind) {
            return;
        }
        if !root_metadata.is_dir() {
            self.add(
                AuditSeverity::High,
                "AUD-DEFINITION-001",
                self.label_path(root),
                format!("{kind} root is not a directory"),
                "Replace it with a protected directory or remove the configuration.",
            );
            return;
        }
        let mut pending = VecDeque::from([(root.to_path_buf(), 0usize)]);
        while let Some((directory, depth)) = pending.pop_front() {
            if depth > MAX_SCAN_DEPTH {
                self.mark_partial(
                    self.label_path(&directory),
                    "directory depth limit reached".into(),
                );
                continue;
            }
            let mut entries = match fs::read_dir(&directory) {
                Ok(entries) => entries.filter_map(Result::ok).collect::<Vec<_>>(),
                Err(_) => {
                    self.mark_partial(
                        self.label_path(&directory),
                        format!("{kind} directory could not be read"),
                    );
                    continue;
                }
            };
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                if self.scanned_entries >= MAX_SCAN_ENTRIES {
                    self.mark_partial(
                        self.label_path(root),
                        "directory entry limit reached".into(),
                    );
                    pending.clear();
                    break;
                }
                self.scanned_entries = self.scanned_entries.saturating_add(1);
                let path = entry.path();
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(_) => {
                        self.mark_partial(
                            self.label_path(&path),
                            format!("{kind} entry changed or became unreadable"),
                        );
                        continue;
                    }
                };
                if !self.inspect_metadata(&path, &metadata, authority, kind) {
                    continue;
                }
                if metadata.is_dir() {
                    pending.push_back((path, depth.saturating_add(1)));
                } else if metadata.is_file() && (!text || is_auditable_text(&path)) {
                    self.scan_text_file(&path, authority, kind);
                }
            }
        }
    }

    fn scan_text_file(&mut self, path: &Path, authority: &Path, kind: &str) -> Option<String> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(_) => {
                self.mark_partial(
                    self.label_path(path),
                    format!("{kind} could not be inspected"),
                );
                return None;
            }
        };
        if !self.inspect_metadata(path, &metadata, authority, kind) || !metadata.is_file() {
            return None;
        }
        if self.scanned_files >= MAX_SCAN_FILES || self.scanned_bytes >= MAX_TOTAL_BYTES {
            self.mark_partial(self.label_path(path), "aggregate scan limit reached".into());
            return None;
        }
        let limit = MAX_FILE_BYTES.min(MAX_TOTAL_BYTES.saturating_sub(self.scanned_bytes));
        let bytes = match read_bounded_nofollow(path, limit) {
            Ok(bytes) => bytes,
            Err(_) => {
                self.mark_partial(
                    self.label_path(path),
                    format!("{kind} could not be read safely"),
                );
                return None;
            }
        };
        self.scanned_files = self.scanned_files.saturating_add(1);
        self.scanned_bytes = self.scanned_bytes.saturating_add(bytes.len().min(limit));
        if bytes.len() > limit {
            self.mark_partial(
                self.label_path(path),
                "individual file byte limit reached".into(),
            );
        }
        let retained = &bytes[..bytes.len().min(limit)];
        let text = match std::str::from_utf8(retained) {
            Ok(text) => text.to_string(),
            Err(_) => {
                self.add(
                    AuditSeverity::Medium,
                    "AUD-DEFINITION-001",
                    self.label_path(path),
                    format!("{kind} is not valid UTF-8 text"),
                    "Replace the harness definition with bounded UTF-8 text.",
                );
                return None;
            }
        };
        self.detect_secret_literals(path, &text);
        Some(text)
    }

    fn inspect_metadata(
        &mut self,
        path: &Path,
        metadata: &fs::Metadata,
        authority: &Path,
        kind: &str,
    ) -> bool {
        let label = self.label_path(path);
        if metadata.file_type().is_symlink() {
            let escapes = fs::canonicalize(path)
                .map(|target| !target.starts_with(authority))
                .unwrap_or(true);
            self.add(
                if escapes {
                    AuditSeverity::Critical
                } else {
                    AuditSeverity::High
                },
                "AUD-SYMLINK-001",
                label,
                if escapes {
                    format!("{kind} symlink escapes its declared authority; target withheld")
                } else {
                    format!("{kind} is a symlink and was not followed")
                },
                "Replace the symlink with an owned regular file or directory inside its authority.",
            );
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.mode() & 0o022 != 0 {
                self.add(
                    AuditSeverity::Critical,
                    "AUD-PERMISSIONS-001",
                    label.clone(),
                    format!("{kind} is writable by group or others"),
                    "Remove group/other write permission from the harness path.",
                );
            }
            self.audit_owner(metadata.uid(), &label, kind);
        }
        true
    }

    #[cfg(unix)]
    fn audit_owner(&mut self, uid: u32, label: &str, kind: &str) {
        let current = unsafe { libc::geteuid() };
        if uid != current && uid != 0 {
            self.add(
                AuditSeverity::High,
                "AUD-OWNERSHIP-001",
                label.to_string(),
                format!("{kind} is owned by an unexpected local account"),
                "Change ownership to the current user or a trusted administrator.",
            );
        }
    }

    fn detect_secret_literals(&mut self, path: &Path, text: &str) {
        for (index, raw_line) in text.lines().enumerate() {
            let line = raw_line.trim();
            let lower = line.to_ascii_lowercase();
            let private_key = lower.contains("-----begin ") && lower.contains("private key-----");
            let prefixed_secret = line.contains("sk-")
                || line.contains("ghp_")
                || line.contains("AKIA")
                || line.contains("xoxb-");
            let assigned_secret = split_assignment(line)
                .is_some_and(|(key, value)| secret_key_name(key) && !safe_secret_reference(value));
            if private_key || prefixed_secret || assigned_secret {
                self.add(
                    AuditSeverity::High,
                    "AUD-SECRET-001",
                    self.label_path(path),
                    format!(
                        "secret-shaped literal detected at line {}; value withheld",
                        index.saturating_add(1)
                    ),
                    "Remove the literal and use an environment or credential-store reference.",
                );
                break;
            }
        }
    }

    fn allowed_skill_root(&self, path: &Path) -> bool {
        if path.starts_with(self.workspace.join("oracles"))
            || path.starts_with(self.workspace.join("reference"))
        {
            return false;
        }
        path.starts_with(&self.workspace)
            || path.starts_with(&self.base_paths.home)
            || path.starts_with(&self.base_paths.config_root)
            || path.starts_with(&self.base_paths.data_dir)
            || path.starts_with("/etc/codex/skills")
    }

    fn authority_for(&self, path: &Path) -> PathBuf {
        if path.starts_with(&self.workspace) {
            self.workspace.clone()
        } else if path.starts_with(&self.base_paths.config_root) {
            self.base_paths.config_root.clone()
        } else if path.starts_with(&self.base_paths.home) {
            self.base_paths.home.clone()
        } else {
            path.parent().unwrap_or(path).to_path_buf()
        }
    }

    fn label_path(&self, path: &Path) -> String {
        let (prefix, relative) = if let Ok(relative) = path.strip_prefix(&self.workspace) {
            ("workspace", relative)
        } else if let Ok(relative) = path.strip_prefix(&self.base_paths.config_root) {
            ("config", relative)
        } else if let Ok(relative) = path.strip_prefix(&self.base_paths.data_dir) {
            ("data", relative)
        } else if let Ok(relative) = path.strip_prefix(&self.base_paths.home) {
            ("home", relative)
        } else {
            ("external", Path::new(path.file_name().unwrap_or_default()))
        };
        let relative = relative.to_string_lossy();
        if relative.is_empty() {
            prefix.into()
        } else {
            safe_text(&format!("{prefix}/{relative}"), 512)
        }
    }

    fn mark_partial(&mut self, path: String, evidence: String) {
        self.partial = true;
        self.add(
            AuditSeverity::Medium,
            "AUD-LIMIT-001",
            path,
            evidence,
            "Reduce the harness surface or repair read access, then rerun the audit.",
        );
    }

    fn add(
        &mut self,
        severity: AuditSeverity,
        rule_id: &'static str,
        path: impl Into<String>,
        evidence: impl Into<String>,
        remediation: &'static str,
    ) {
        let path = safe_text(&path.into(), 512);
        let evidence = safe_text(&evidence.into(), 1024);
        let key = format!("{}\0{}\0{}", rule_id, path, evidence);
        if !self.finding_keys.insert(key) {
            return;
        }
        if self.findings.len() >= MAX_FINDINGS {
            self.partial = true;
            if !self
                .findings
                .iter()
                .any(|finding| finding.rule_id == "AUD-LIMIT-001" && finding.path == "scan")
            {
                self.findings.pop();
                self.findings.push(AuditFinding {
                    severity: AuditSeverity::Medium,
                    rule_id: "AUD-LIMIT-001",
                    path: "scan".into(),
                    evidence: "finding count limit reached; additional findings omitted".into(),
                    remediation:
                        "Reduce the harness surface or repair read access, then rerun the audit.",
                });
            }
            return;
        }
        self.findings.push(AuditFinding {
            severity,
            rule_id,
            path,
            evidence,
            remediation,
        });
    }
}

fn permission_is_broad(matcher: &crate::permission::PermissionMatcher) -> bool {
    match matcher {
        crate::permission::PermissionMatcher::Tool { tool } => matches!(
            tool.as_str(),
            "run_command"
                | "write_file"
                | "edit_file"
                | "hashline_edit"
                | "apply_patch"
                | "delete_file"
                | "browser"
        ),
        crate::permission::PermissionMatcher::Path { path_pattern, .. } => {
            matches!(path_pattern.as_str(), "/**" | "/*" | "/")
                || path_pattern.ends_with("/**") && path_pattern.matches('/').count() <= 2
        }
        crate::permission::PermissionMatcher::CommandPrefix { prefix } => prefix.len() <= 1,
        crate::permission::PermissionMatcher::Invocation { .. } => false,
    }
}

fn legacy_permission_is_broad(rule: &crate::config::ApprovalRule) -> bool {
    match rule {
        crate::config::ApprovalRule::AllowTool(tool) => matches!(
            tool.as_str(),
            "run_command"
                | "write_file"
                | "edit_file"
                | "hashline_edit"
                | "apply_patch"
                | "delete_file"
        ),
        crate::config::ApprovalRule::AllowPath { path_pattern, .. } => {
            matches!(path_pattern.as_str(), "/**" | "/*" | "/")
        }
        crate::config::ApprovalRule::DenyTool(_) | crate::config::ApprovalRule::DenyPath { .. } => {
            false
        }
    }
}

fn contains_shell_composition(command: &str) -> bool {
    command
        .chars()
        .any(|character| matches!(character, '|' | ';' | '&' | '`' | '\n' | '\r' | '\0'))
        || command.contains("$(")
        || command.contains("${")
        || command.contains('>')
        || command.contains('<')
}

fn first_command_token(command: &str) -> Option<String> {
    let command = command.trim_start();
    if command.is_empty() {
        return None;
    }
    let mut token = String::new();
    let mut quote = None;
    for character in command.chars() {
        match (quote, character) {
            (None, '\'' | '"') => quote = Some(character),
            (Some(expected), current) if expected == current => quote = None,
            (None, current) if current.is_whitespace() => break,
            (_, current) => token.push(current),
        }
    }
    (quote.is_none() && !token.is_empty()).then_some(token)
}

fn resolve_in_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .filter(|directory| directory.is_absolute())
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn is_auditable_text(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("md" | "toml" | "json" | "yaml" | "yml" | "txt" | "sh" | "py" | "js" | "ts")
    ) || path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md")
}

fn split_assignment(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=').or_else(|| line.split_once(':'))?;
    let key = key.trim().trim_matches(['"', '\'']);
    let value = value.trim().trim_matches([',', '"', '\'', ' ']);
    (!key.is_empty() && !value.is_empty()).then_some((key, value))
}

fn secret_key_name(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    normalized == "authorization"
        || normalized == "cookie"
        || normalized == "api_key"
        || normalized.ends_with("_api_key")
        || normalized == "token"
        || normalized.ends_with("_token")
        || normalized == "password"
        || normalized.ends_with("_password")
        || normalized == "client_secret"
        || normalized.ends_with("_private_key")
}

fn safe_secret_reference(value: &str) -> bool {
    if value.is_empty()
        || value == "ollama"
        || crate::provider_secrets::resembles_saved_reference(value)
    {
        return true;
    }
    value
        .strip_prefix("${")
        .and_then(|value| value.strip_suffix('}'))
        .is_some_and(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        })
}

fn read_bounded_nofollow(path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?
    };
    #[cfg(not(unix))]
    let file = OpenOptions::new().read(true).open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "not a regular file",
        ));
    }
    let mut bytes = Vec::with_capacity(limit.min(metadata.len() as usize));
    let mut reader: Take<File> = file.take(limit.saturating_add(1) as u64);
    reader.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn safe_text(value: &str, maximum: usize) -> String {
    let sanitized = crate::terminal_text::sanitize_terminal_text(value);
    let mut end = maximum.min(sanitized.len());
    while end > 0 && !sanitized.is_char_boundary(end) {
        end -= 1;
    }
    let mut output = sanitized[..end].to_string();
    if end < sanitized.len() {
        output.push('…');
    }
    output
}

fn bound_report(mut output: String) -> String {
    if output.len() <= MAX_REPORT_BYTES {
        return output;
    }
    const MARKER: &str = "\n\n[… audit report truncated at the 64 KiB display limit …]";
    let mut end = MAX_REPORT_BYTES.saturating_sub(MARKER.len());
    while end > 0 && !output.is_char_boundary(end) {
        end -= 1;
    }
    output.truncate(end);
    output.push_str(MARKER);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn request(root: &std::path::Path, home: &std::path::Path, config: CaConfig) -> AuditRequest {
        AuditRequest {
            workspace_root: root.to_path_buf(),
            cwd: root.to_path_buf(),
            runtime_paths: RuntimePaths::from_home(home),
            profile: ActiveProfile::Default,
            loaded_config: Some(config),
        }
    }

    #[test]
    fn security_audit_is_deterministic_and_never_renders_secret_values() {
        let fixture = tempfile::tempdir().unwrap();
        let workspace = fixture.path().join("workspace");
        let home = fixture.path().join("home");
        fs::create_dir_all(workspace.join(".ca/rules")).unwrap();
        fs::create_dir_all(home.join(".config/ca")).unwrap();
        fs::write(
            workspace.join("AGENTS.md"),
            "api_key = \"SEC-120-SENTINEL\"\n",
        )
        .unwrap();
        fs::write(
            home.join(".config/ca/config.toml"),
            "[providers.demo]\nbase_url='https://example.invalid/v1'\napi_key='SEC-120-CONFIG-SENTINEL'\nmodels=['m']\n",
        )
        .unwrap();
        let mut config = CaConfig::default();
        config
            .permission_rules
            .push(crate::permission::PermissionRule {
                decision: crate::permission::PermissionDecision::Allow,
                matcher: crate::permission::PermissionMatcher::Tool {
                    tool: "run_command".into(),
                },
            });
        config.hooks.push(crate::config::HookConfig {
            event: "pre_tool_use".into(),
            command: "helper | sh".into(),
            enabled: true,
            timeout_secs: 30,
            fail_closed: false,
        });

        let first = run(request(&workspace, &home, config.clone())).unwrap();
        let second = run(request(&workspace, &home, config)).unwrap();
        let rendered = first.render_text();
        assert_eq!(first, second);
        assert!(first.has_high_or_critical());
        assert!(rendered.contains("AUD-SECRET-001"));
        assert!(rendered.contains("AUD-PERM-001"));
        assert!(rendered.contains("AUD-EXEC-001"));
        assert!(!rendered.contains("SEC-120-SENTINEL"));
        assert!(!rendered.contains("SEC-120-CONFIG-SENTINEL"));
    }

    #[cfg(unix)]
    #[test]
    fn security_audit_reports_symlink_escape_without_reading_target() {
        use std::os::unix::fs::symlink;

        let fixture = tempfile::tempdir().unwrap();
        let workspace = fixture.path().join("workspace");
        let home = fixture.path().join("home");
        let outside = fixture.path().join("outside");
        fs::create_dir_all(workspace.join(".agents/skills")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("SKILL.md"), "ESCAPE-SECRET-SENTINEL").unwrap();
        symlink(&outside, workspace.join(".agents/skills/escaped")).unwrap();

        let report = run(request(&workspace, &home, CaConfig::default())).unwrap();
        let rendered = report.render_text();
        assert!(rendered.contains("AUD-SYMLINK-001"));
        assert!(!rendered.contains("ESCAPE-SECRET-SENTINEL"));
    }

    #[test]
    fn security_audit_marks_limit_exhaustion_partial_and_bounded() {
        let report = AuditReport {
            workspace: "workspace".into(),
            profile: "default".into(),
            scanned_files: 512,
            scanned_bytes: 2 * 1024 * 1024,
            partial: true,
            findings: vec![AuditFinding {
                severity: AuditSeverity::Medium,
                rule_id: "AUD-LIMIT-001",
                path: "workspace".into(),
                evidence: "file-count limit reached".into(),
                remediation: "Reduce the harness surface and rerun the audit.",
            }],
        };
        let rendered = report.render_text();
        assert!(rendered.contains("PARTIAL"));
        assert!(rendered.len() < 64 * 1024);
    }

    #[cfg(unix)]
    #[test]
    fn security_audit_reports_writable_harness_files_and_sanitizes_controls() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = tempfile::tempdir().unwrap();
        let workspace = fixture.path().join("workspace");
        let home = fixture.path().join("home");
        fs::create_dir_all(&workspace).unwrap();
        let instruction = workspace.join("AGENTS.md");
        fs::write(&instruction, "ordinary instruction\n").unwrap();
        fs::set_permissions(&instruction, fs::Permissions::from_mode(0o666)).unwrap();

        let mut report = run(request(&workspace, &home, CaConfig::default())).unwrap();
        report.findings.push(AuditFinding {
            severity: AuditSeverity::Low,
            rule_id: "AUD-TEST-001",
            path: "workspace/control\u{1b}[31m".into(),
            evidence: "control\u{7} evidence".into(),
            remediation: "Use plain text.",
        });
        let rendered = report.render_text();
        assert!(rendered.contains("AUD-PERMISSIONS-001"));
        assert!(!rendered.contains('\u{1b}'));
        assert!(!rendered.contains('\u{7}'));
    }

    #[test]
    fn security_audit_marks_invalid_profile_configuration_without_mutating_it() {
        let fixture = tempfile::tempdir().unwrap();
        let workspace = fixture.path().join("workspace");
        let home = fixture.path().join("home");
        let paths = RuntimePaths::from_home(&home);
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
        let invalid = b"[mcp.servers.bad\ncommand = 'never'\n";
        fs::write(&paths.config_file, invalid).unwrap();

        let report = run(AuditRequest {
            workspace_root: workspace.clone(),
            cwd: workspace,
            runtime_paths: paths.clone(),
            profile: ActiveProfile::Default,
            loaded_config: None,
        })
        .unwrap();
        assert!(report
            .findings
            .iter()
            .any(|finding| finding.rule_id == "AUD-CONFIG-001"));
        assert_eq!(fs::read(&paths.config_file).unwrap(), invalid);
    }
}
