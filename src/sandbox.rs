use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SandboxMode {
    Required,
    PolicyOnly,
    Off,
}

impl SandboxMode {
    pub fn parse(value: &str) -> Self {
        match value {
            "policy-only" | "policy_only" => Self::PolicyOnly,
            "off" => Self::Off,
            _ => Self::Required,
        }
    }

    pub fn from_saved_value(value: &str) -> Result<Self, SandboxModeParseError> {
        match value {
            "required" => Ok(Self::Required),
            "policy-only" => Ok(Self::PolicyOnly),
            "off" => Ok(Self::Off),
            _ => Err(SandboxModeParseError),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::PolicyOnly => "policy-only",
            Self::Off => "off",
        }
    }

    pub const fn is_weaker_than(self, other: Self) -> bool {
        self.strength() < other.strength()
    }

    const fn strength(self) -> u8 {
        match self {
            Self::Required => 2,
            Self::PolicyOnly => 1,
            Self::Off => 0,
        }
    }
}

impl std::fmt::Display for SandboxMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq, Eq)]
#[error("sandbox mode must be required, policy-only, or off")]
pub struct SandboxModeParseError;

#[derive(Debug, Clone)]
pub struct SandboxPolicy {
    pub roots: Vec<PathBuf>,
    /// Additional roots that may be read but never written. This is used by
    /// isolated experiment commands for an installed compiler toolchain.
    pub read_only_roots: Vec<PathBuf>,
    pub temporary_directory: PathBuf,
    pub network_allowed: bool,
    /// Deny process enumeration and sysctl inspection to sandboxed children.
    /// Process creation remains available so compilers and test runners work.
    pub hide_other_processes: bool,
}

#[derive(Debug, Clone)]
pub struct PreparedProcess {
    pub program: PathBuf,
    pub arguments: Vec<String>,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum SandboxError {
    #[error("sandbox-required process execution is unavailable on this platform")]
    Unavailable,
    #[error("sandbox policy path contains an unsupported character: {0}")]
    InvalidPath(String),
    #[error("process executable is outside sandbox-readable locations: {0}")]
    ExecutableOutsidePolicy(String),
    #[cfg(target_os = "freebsd")]
    #[error("native FreeBSD sandbox policy could not be prepared: {0}")]
    NativePolicy(String),
}

#[derive(Debug, Clone)]
pub struct SandboxBackend {
    mode: SandboxMode,
    executable: PathBuf,
    #[cfg(all(test, target_os = "freebsd"))]
    test_executor: bool,
}

impl SandboxBackend {
    pub fn new(mode: SandboxMode) -> Self {
        Self {
            mode,
            executable: PathBuf::from("/usr/bin/sandbox-exec"),
            #[cfg(all(test, target_os = "freebsd"))]
            test_executor: false,
        }
    }

    #[cfg(test)]
    pub fn with_executable(mode: SandboxMode, executable: PathBuf) -> Self {
        Self {
            mode,
            executable,
            #[cfg(target_os = "freebsd")]
            test_executor: true,
        }
    }

    pub fn is_available(&self) -> bool {
        #[cfg(target_os = "freebsd")]
        {
            if self.use_native_freebsd() {
                ca_automation::freebsd_sandbox::available()
            } else {
                self.seatbelt_available()
            }
        }
        #[cfg(not(target_os = "freebsd"))]
        {
            self.seatbelt_available()
        }
    }

    fn seatbelt_available(&self) -> bool {
        #[cfg(all(test, target_os = "freebsd"))]
        if self.test_executor {
            return self.executable.is_file();
        }
        cfg!(target_os = "macos") && self.executable.is_file()
    }

    #[cfg(target_os = "freebsd")]
    fn use_native_freebsd(&self) -> bool {
        #[cfg(test)]
        {
            !self.test_executor
        }
        #[cfg(not(test))]
        {
            true
        }
    }

    pub fn prepare_shell(
        &self,
        command: &str,
        policy: &SandboxPolicy,
    ) -> Result<PreparedProcess, SandboxError> {
        if self.mode != SandboxMode::Required {
            return Ok(PreparedProcess {
                program: PathBuf::from("/bin/sh"),
                arguments: vec!["-c".into(), command.into()],
            });
        }
        #[cfg(target_os = "freebsd")]
        if self.use_native_freebsd() {
            return self.prepare_freebsd(
                Path::new("/bin/sh"),
                &["-c".into(), command.into()],
                policy,
                &[],
            );
        }
        if !self.seatbelt_available() {
            return Err(SandboxError::Unavailable);
        }
        let profile = sandbox_profile(policy)?;
        Ok(PreparedProcess {
            program: self.executable.clone(),
            arguments: vec![
                "-p".into(),
                profile,
                "/bin/sh".into(),
                "-c".into(),
                command.into(),
            ],
        })
    }

    pub fn prepare_process(
        &self,
        program: &Path,
        arguments: &[String],
        policy: &SandboxPolicy,
    ) -> Result<PreparedProcess, SandboxError> {
        self.prepare_process_with_denied_paths(program, arguments, policy, &[])
    }

    /// Prepare a process while keeping private paths inaccessible to the
    /// sandboxed child, even when a broader policy root also covers them.
    ///
    /// In required mode, each denial is emitted after the ordinary allow
    /// rules for both the supplied path and its macOS `/private` alias. In
    /// weaker modes this preserves [`Self::prepare_process`] semantics and
    /// returns the process unchanged.
    pub fn prepare_process_with_denied_paths(
        &self,
        program: &Path,
        arguments: &[String],
        policy: &SandboxPolicy,
        denied_paths: &[PathBuf],
    ) -> Result<PreparedProcess, SandboxError> {
        if self.mode != SandboxMode::Required {
            return Ok(PreparedProcess {
                program: program.to_path_buf(),
                arguments: arguments.to_vec(),
            });
        }
        #[cfg(target_os = "freebsd")]
        if self.use_native_freebsd() {
            return self.prepare_freebsd(program, arguments, policy, denied_paths);
        }
        if !self.seatbelt_available() {
            return Err(SandboxError::Unavailable);
        }
        let mut profile = sandbox_profile(policy)?;
        let canonical_program = program
            .canonicalize()
            .unwrap_or_else(|_| program.to_path_buf());
        let covered = policy
            .roots
            .iter()
            .chain(&policy.read_only_roots)
            .chain([&policy.temporary_directory])
            .any(|root| canonical_program.starts_with(root))
            || ["/System", "/usr", "/bin", "/sbin", "/Library"]
                .iter()
                .any(|root| canonical_program.starts_with(root));
        if !covered {
            let read_root = [Path::new("/opt/homebrew"), Path::new("/usr/local")]
                .into_iter()
                .find(|root| canonical_program.starts_with(root))
                .ok_or_else(|| {
                    SandboxError::ExecutableOutsidePolicy(canonical_program.display().to_string())
                })?;
            let literal = sandbox_literal(read_root)?;
            for ancestor in read_root.ancestors().skip(1) {
                if ancestor == Path::new("/") {
                    continue;
                }
                let ancestor = sandbox_literal(ancestor)?;
                profile.push_str(&format!(
                    "(allow file-read-metadata (literal \"{ancestor}\"))\n"
                ));
            }
            profile.push_str(&format!("(allow file-read* (subpath \"{literal}\"))\n"));
        }
        append_denied_path_rules(&mut profile, denied_paths)?;
        let mut prepared_arguments = vec!["-p".into(), profile, program.to_string_lossy().into()];
        prepared_arguments.extend(arguments.iter().cloned());
        Ok(PreparedProcess {
            program: self.executable.clone(),
            arguments: prepared_arguments,
        })
    }

    #[cfg(target_os = "freebsd")]
    fn prepare_freebsd(
        &self,
        program: &Path,
        arguments: &[String],
        policy: &SandboxPolicy,
        denied_paths: &[PathBuf],
    ) -> Result<PreparedProcess, SandboxError> {
        let mut writable = policy.roots.clone();
        writable.push(policy.temporary_directory.clone());
        let mut readable = ca_automation::freebsd_sandbox::system_reads();
        readable.extend_from_slice(&policy.read_only_roots);
        let (program, arguments) = ca_automation::freebsd_sandbox::prepare_process(
            program,
            arguments,
            &writable,
            &readable,
            denied_paths,
            policy.network_allowed,
        )
        .map_err(|error| SandboxError::NativePolicy(error.to_string()))?;
        Ok(PreparedProcess { program, arguments })
    }
}

pub(crate) fn sandbox_profile(policy: &SandboxPolicy) -> Result<String, SandboxError> {
    let mut profile = String::from(
        "(version 1)\n(deny default)\n(allow process*)\n(allow sysctl-read)\n\
         (allow file-read* (subpath \"/System\") (subpath \"/usr\") (subpath \"/bin\") \
         (subpath \"/sbin\") (subpath \"/Library\") (subpath \"/private/etc\") \
         (subpath \"/private/var/select\") (subpath \"/var/folders\") \
         (literal \"/\") (literal \"/var\") (literal \"/tmp\") \
         (literal \"/var/db/xcode_select_link\") \
         (literal \"/dev/null\") (literal \"/dev/urandom\"))\n\
         (allow file-read-metadata (literal \"/var/db\"))\n\
         (allow file-read-metadata (literal \"/var/db/xcode_select_link\"))\n\
         (allow file-write* (literal \"/dev/null\"))\n",
    );
    if !policy.hide_other_processes || policy.network_allowed {
        // macOS libc resolves uid/gid records through OpenDirectory rather
        // than directly from /etc/passwd. Without this one libinfo lookup,
        // ordinary tools such as ssh fail before connecting with "No user
        // exists for uid …". Network-authorized automation commands also need
        // this identity lookup while process inspection stays denied.
        profile.push_str(
            "(allow file-read-metadata (literal \"/etc\"))\n\
             (allow mach-lookup\n\
             (global-name \"com.apple.system.opendirectoryd.libinfo\"))\n",
        );
    }
    if policy.hide_other_processes {
        // Cargo and the Rust runtime need only their own process identity and
        // compatible page size and CPU count (sysconf's available parallelism).
        // Keep every other process-info and
        // sysctl surface denied so an experiment child cannot inspect its
        // credential-holding parent or host state.
        profile.push_str(
            "(allow signal (target same-sandbox))\n\
             (deny process-info*)\n\
             (allow process-info* (target self))\n\
             (deny sysctl-read)\n\
             (allow sysctl-read (sysctl-name \"hw.pagesize_compat\"))\n\
             (allow sysctl-read (sysctl-name \"hw.ncpu\"))\n",
        );
    }
    for root in policy.roots.iter().chain([&policy.temporary_directory]) {
        for path in sandbox_path_aliases(root) {
            append_ancestor_metadata_rules(&mut profile, &path)?;
            let literal = sandbox_literal(&path)?;
            profile.push_str(&format!(
                "(allow file-read* (subpath \"{literal}\"))\n(allow file-write* (subpath \"{literal}\"))\n"
            ));
        }
    }
    for root in &policy.read_only_roots {
        for path in sandbox_path_aliases(root) {
            append_ancestor_metadata_rules(&mut profile, &path)?;
            let literal = sandbox_literal(&path)?;
            if path.is_file() {
                profile.push_str(&format!("(allow file-read* (literal \"{literal}\"))\n"));
            } else {
                profile.push_str(&format!("(allow file-read* (subpath \"{literal}\"))\n"));
            }
        }
    }
    if !policy.hide_other_processes {
        // xcodebuild and swiftc call confstr for the Darwin user cache and
        // temp directories, and Xcode watches files through FSEvents. Without
        // these lookups the compiler logs EIO, then fails to write
        // ~/.cache/clang/ModuleCache. The write rules cover only xcrun's
        // database and Swift's TemporaryDirectory, not the rest of the user
        // temporary folder.
        profile.push_str(
            "(allow mach-lookup (global-name \"com.apple.bsd.dirhelper\"))\n\
             (allow mach-lookup (global-name \"com.apple.FSEvents\"))\n\
             (allow file-write*\n\
             (regex #\"^/(private/)?var/folders/[^/]+/[^/]+/T/xcrun_db\")\n\
             (regex #\"^/(private/)?var/folders/[^/]+/[^/]+/T/TemporaryDirectory\\.\"))\n",
        );
    }
    // Ordinary jobs may use conventional /tmp paths, but host-owned privilege
    // handoffs always remain invisible even when the parent environment places
    // its private directory below a writable temporary root.
    profile.push_str(
        "(deny file-read* file-write*\n\
         (regex #\"^/(private/)?tmp/ca-privilege-[^/]+(/|$)\")\n\
         (regex #\"^/(private/)?var/folders/.*/T/ca-privilege-[^/]+(/|$)\"))\n",
    );
    if policy.network_allowed {
        profile.push_str("(allow network*)\n");
        append_credential_rules(
            &mut profile,
            user_keychains_directory(),
            ssh_agent_socket_directory(),
            user_home_directory(),
        )?;
    } else {
        profile.push_str("(deny network*)\n");
    }
    Ok(profile)
}

fn append_credential_rules(
    profile: &mut String,
    keychains_directory: Option<PathBuf>,
    ssh_agent_socket_directory: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<(), SandboxError> {
    profile.push_str(
        "(allow mach-lookup\n\
         (global-name \"com.apple.securityd\"))\n",
    );
    if let Some(keychains) = keychains_directory {
        let literal = sandbox_literal(&keychains)?;
        profile.push_str(&format!("(allow file-read* (subpath \"{literal}\"))\n"));
    }
    if let Some(socket) = ssh_agent_socket_directory {
        for path in sandbox_path_aliases(&socket) {
            append_ancestor_metadata_rules(profile, &path)?;
            let literal = sandbox_literal(&path)?;
            profile.push_str(&format!(
                "(allow file-read* file-write* (subpath \"{literal}\"))\n"
            ));
        }
    }
    if let Some(home) = home {
        append_host_vcs_credential_rules(profile, &home)?;
    }
    Ok(())
}

/// Git/GitHub/SSH work needs both network and the user's conventional
/// credential surfaces. Reads of the whole home tree are already covered by
/// the authorized-command `/` root; these rules add the write side that
/// `known_hosts`, `gh` config, and git credential helpers require.
fn append_host_vcs_credential_rules(profile: &mut String, home: &Path) -> Result<(), SandboxError> {
    for relative in [
        ".ssh",
        ".config/gh",
        ".config/git",
        "Library/Application Support/gh",
    ] {
        let path = home.join(relative);
        for alias in sandbox_path_aliases(&path) {
            append_ancestor_metadata_rules(profile, &alias)?;
            let literal = sandbox_literal(&alias)?;
            profile.push_str(&format!(
                "(allow file-read* file-write* (subpath \"{literal}\"))\n"
            ));
        }
    }
    for relative in [".gitconfig", ".git-credentials"] {
        let path = home.join(relative);
        for alias in sandbox_path_aliases(&path) {
            append_ancestor_metadata_rules(profile, &alias)?;
            let literal = sandbox_literal(&alias)?;
            profile.push_str(&format!(
                "(allow file-read* file-write* (literal \"{literal}\"))\n"
            ));
        }
    }
    Ok(())
}

fn user_home_directory() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn user_keychains_directory() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(Path::new(&home).join("Library").join("Keychains"))
}

fn ssh_agent_socket_directory() -> Option<PathBuf> {
    let socket = std::env::var_os("SSH_AUTH_SOCK")?;
    Path::new(&socket).parent().map(Path::to_path_buf)
}

pub(crate) fn append_denied_path_rules(
    profile: &mut String,
    denied_paths: &[PathBuf],
) -> Result<(), SandboxError> {
    for root in denied_paths {
        for path in sandbox_path_aliases(root) {
            let literal = sandbox_literal(&path)?;
            profile.push_str(&format!(
                "(deny file-read* file-write* (literal \"{literal}\") (subpath \"{literal}\"))\n"
            ));
        }
    }
    Ok(())
}

fn append_ancestor_metadata_rules(profile: &mut String, path: &Path) -> Result<(), SandboxError> {
    let mut ancestors = path.ancestors().skip(1).collect::<Vec<_>>();
    ancestors.reverse();
    for ancestor in ancestors {
        if ancestor == Path::new("/") {
            continue;
        }
        let literal = sandbox_literal(ancestor)?;
        profile.push_str(&format!(
            "(allow file-read-metadata (literal \"{literal}\"))\n"
        ));
    }
    Ok(())
}

fn sandbox_path_aliases(path: &Path) -> Vec<PathBuf> {
    let mut paths = vec![path.to_path_buf()];
    let value = path.to_string_lossy();
    if value == "/private/var" {
        paths.push(PathBuf::from("/var"));
    } else if value == "/private/tmp" {
        paths.push(PathBuf::from("/tmp"));
    } else if value == "/var" {
        paths.push(PathBuf::from("/private/var"));
    } else if value == "/tmp" {
        paths.push(PathBuf::from("/private/tmp"));
    } else if let Some(alias) = value.strip_prefix("/private/var/") {
        paths.push(PathBuf::from("/var").join(alias));
    } else if let Some(alias) = value.strip_prefix("/private/tmp/") {
        paths.push(PathBuf::from("/tmp").join(alias));
    } else if let Some(alias) = value.strip_prefix("/var/") {
        paths.push(PathBuf::from("/private/var").join(alias));
    } else if let Some(alias) = value.strip_prefix("/tmp/") {
        paths.push(PathBuf::from("/private/tmp").join(alias));
    }
    paths
}

fn sandbox_literal(path: &Path) -> Result<String, SandboxError> {
    let value = path.to_string_lossy();
    if value.contains(['\n', '\r', '\0']) {
        return Err(SandboxError::InvalidPath(value.into_owned()));
    }
    Ok(value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_backend_fails_when_capability_is_missing() {
        let backend = SandboxBackend::with_executable(
            SandboxMode::Required,
            PathBuf::from("/definitely/missing/sandbox-exec"),
        );
        let policy = SandboxPolicy {
            roots: vec![PathBuf::from("/tmp/workspace")],
            read_only_roots: Vec::new(),
            temporary_directory: PathBuf::from("/tmp/job"),
            network_allowed: false,
            hide_other_processes: false,
        };
        assert_eq!(
            backend.prepare_shell("true", &policy).unwrap_err(),
            SandboxError::Unavailable
        );
    }

    #[test]
    #[cfg(target_os = "freebsd")]
    fn required_backend_uses_the_native_helper_or_fails_closed() {
        let backend = SandboxBackend::new(SandboxMode::Required);
        let prepared = backend.prepare_shell(
            "true",
            &SandboxPolicy {
                roots: vec![PathBuf::from("/tmp/workspace")],
                read_only_roots: Vec::new(),
                temporary_directory: PathBuf::from("/tmp/job"),
                network_allowed: false,
                hide_other_processes: true,
            },
        );
        let helper = Path::new(ca_automation::freebsd_sandbox::HELPER_PATH);
        if !helper.exists() {
            assert!(matches!(prepared, Err(SandboxError::NativePolicy(_))));
            return;
        }
        let prepared = prepared.unwrap();
        assert_eq!(prepared.program, helper);
        assert_eq!(&prepared.arguments[..2], ["run", "--permissions-json"]);
        assert_eq!(prepared.arguments[3..], ["--", "/bin/sh", "-c", "true"]);
        let policy: serde_json::Value = serde_json::from_str(&prepared.arguments[2]).unwrap();
        assert_eq!(policy["network"], "restricted");
    }

    #[test]
    fn policy_only_is_explicit_and_does_not_claim_a_sandbox() {
        let backend = SandboxBackend::new(SandboxMode::PolicyOnly);
        let process = backend
            .prepare_shell(
                "true",
                &SandboxPolicy {
                    roots: Vec::new(),
                    read_only_roots: Vec::new(),
                    temporary_directory: PathBuf::from("/tmp/job"),
                    network_allowed: false,
                    hide_other_processes: false,
                },
            )
            .unwrap();
        assert_eq!(process.program, PathBuf::from("/bin/sh"));
    }

    #[test]
    fn direct_process_preserves_program_and_argument_boundaries() {
        let backend = SandboxBackend::new(SandboxMode::PolicyOnly);
        let process = backend
            .prepare_process(
                Path::new("/bin/echo"),
                &["a b".into(), "; false".into()],
                &SandboxPolicy {
                    roots: Vec::new(),
                    read_only_roots: Vec::new(),
                    temporary_directory: PathBuf::from("/tmp/job"),
                    network_allowed: false,
                    hide_other_processes: false,
                },
            )
            .unwrap();
        assert_eq!(process.program, PathBuf::from("/bin/echo"));
        assert_eq!(process.arguments, vec!["a b", "; false"]);
    }

    #[test]
    fn macos_private_path_aliases_are_both_scoped() {
        assert_eq!(
            sandbox_path_aliases(Path::new("/private/var/tmp/work")),
            vec![
                PathBuf::from("/private/var/tmp/work"),
                PathBuf::from("/var/tmp/work")
            ]
        );
        assert_eq!(
            sandbox_path_aliases(Path::new("/tmp/work")),
            vec![
                PathBuf::from("/tmp/work"),
                PathBuf::from("/private/tmp/work")
            ]
        );
        assert_eq!(
            sandbox_path_aliases(Path::new("/private/tmp")),
            vec![PathBuf::from("/private/tmp"), PathBuf::from("/tmp")]
        );
        assert_eq!(
            sandbox_path_aliases(Path::new("/tmp")),
            vec![PathBuf::from("/tmp"), PathBuf::from("/private/tmp")]
        );
    }

    #[test]
    fn private_path_denials_follow_root_allow_and_cover_macos_alias() {
        let private_directory = PathBuf::from("/tmp/ca-private-handoff");
        let mut profile = sandbox_profile(&SandboxPolicy {
            roots: vec![PathBuf::from("/")],
            read_only_roots: Vec::new(),
            temporary_directory: PathBuf::from("/tmp/job"),
            network_allowed: false,
            hide_other_processes: false,
        })
        .unwrap();
        append_denied_path_rules(&mut profile, std::slice::from_ref(&private_directory)).unwrap();

        let root_allow = "(allow file-read* (subpath \"/\"))";
        let direct_deny = "(deny file-read* file-write* (literal \"/tmp/ca-private-handoff\") (subpath \"/tmp/ca-private-handoff\"))";
        let alias_deny = "(deny file-read* file-write* (literal \"/private/tmp/ca-private-handoff\") (subpath \"/private/tmp/ca-private-handoff\"))";
        let root_allow_offset = profile.find(root_allow).unwrap();
        let direct_deny_offset = profile.find(direct_deny).unwrap();
        let alias_deny_offset = profile.find(alias_deny).unwrap();

        assert!(root_allow_offset < direct_deny_offset);
        assert!(direct_deny_offset < alias_deny_offset);
    }

    #[test]
    fn conventional_temp_writes_keep_privilege_handoff_prefixes_denied() {
        let profile = sandbox_profile(&SandboxPolicy {
            roots: vec![PathBuf::from("/tmp/workspace"), PathBuf::from("/tmp")],
            read_only_roots: vec![PathBuf::from("/")],
            temporary_directory: PathBuf::from("/tmp/ca-job-private"),
            network_allowed: false,
            hide_other_processes: false,
        })
        .unwrap();
        let temp_allow = "(allow file-write* (subpath \"/tmp\"))";
        let privilege_deny = "(regex #\"^/(private/)?tmp/ca-privilege-[^/]+(/|$)\")";
        assert!(profile.contains(temp_allow));
        assert!(profile.contains(privilege_deny));
        assert!(profile.find(temp_allow).unwrap() < profile.find(privilege_deny).unwrap());
    }

    #[test]
    fn explicit_read_only_files_use_literal_not_directory_rules() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("approved-tool");
        std::fs::write(&executable, "fixture").unwrap();
        let profile = sandbox_profile(&SandboxPolicy {
            roots: Vec::new(),
            read_only_roots: vec![executable.clone()],
            temporary_directory: root.path().join("tmp"),
            network_allowed: false,
            hide_other_processes: false,
        })
        .unwrap();
        let escaped = sandbox_literal(&executable).unwrap();
        assert!(profile.contains(&format!("(allow file-read* (literal \"{escaped}\"))")));
        assert!(!profile.contains(&format!("(allow file-read* (subpath \"{escaped}\"))")));
    }

    #[test]
    fn editable_modes_accept_only_canonical_values() {
        assert_eq!(
            SandboxMode::from_saved_value("required").unwrap(),
            SandboxMode::Required
        );
        assert_eq!(
            SandboxMode::from_saved_value("policy-only").unwrap(),
            SandboxMode::PolicyOnly
        );
        assert_eq!(
            SandboxMode::from_saved_value("off").unwrap(),
            SandboxMode::Off
        );
        assert!(SandboxMode::from_saved_value("policy_only").is_err());
        assert!(SandboxMode::from_saved_value("unknown").is_err());
        assert_eq!(SandboxMode::parse("unknown"), SandboxMode::Required);
    }

    #[test]
    fn experiment_profile_denies_process_inspection_without_disabling_exec() {
        let profile = sandbox_profile(&SandboxPolicy {
            roots: vec![PathBuf::from("/tmp/workspace")],
            read_only_roots: vec![PathBuf::from("/opt/homebrew")],
            temporary_directory: PathBuf::from("/tmp/job"),
            network_allowed: false,
            hide_other_processes: true,
        })
        .unwrap();
        assert!(profile.contains("(allow process*)"));
        assert!(profile.contains("(deny process-info*)"));
        assert!(profile.contains("(allow process-info* (target self))"));
        assert!(profile.contains("(deny sysctl-read)"));
        assert!(profile.contains("(allow sysctl-read (sysctl-name \"hw.pagesize_compat\"))"));
        assert!(profile.contains("(deny network*)"));
        assert!(profile.contains("(literal \"/var/db/xcode_select_link\")"));
        assert!(profile.contains("(allow file-write* (literal \"/dev/null\"))"));
        assert!(profile.contains("(allow file-read* (subpath \"/opt/homebrew\"))"));
        assert!(!profile.contains("(allow file-write* (subpath \"/opt/homebrew\"))"));
        assert!(!profile.contains("com.apple.system.opendirectoryd.libinfo"));
    }

    #[test]
    fn ordinary_profile_allows_only_libinfo_identity_lookup() {
        let profile = sandbox_profile(&SandboxPolicy {
            roots: vec![PathBuf::from("/tmp/workspace")],
            read_only_roots: Vec::new(),
            temporary_directory: PathBuf::from("/tmp/job"),
            network_allowed: false,
            hide_other_processes: false,
        })
        .unwrap();
        assert!(profile.contains("com.apple.system.opendirectoryd.libinfo"));
        assert!(profile.contains("(allow file-read-metadata (literal \"/etc\"))"));
        assert!(profile.contains("com.apple.bsd.dirhelper"));
        assert!(profile.contains("com.apple.FSEvents"));
        assert!(profile.contains("TemporaryDirectory\\."));
        assert!(profile.contains("xcrun_db"));
        assert!(!profile.contains("(allow file-write* (subpath \"/etc\"))"));
        assert!(!profile.contains("com.apple.system.opendirectoryd.api"));
        assert!(!profile.contains("com.apple.system.opendirectoryd.membership"));
    }

    #[test]
    fn network_authorized_private_worker_resolves_identity_without_process_inspection() {
        let profile = sandbox_profile(&SandboxPolicy {
            roots: vec![PathBuf::from("/tmp/workspace")],
            read_only_roots: Vec::new(),
            temporary_directory: PathBuf::from("/tmp/job"),
            network_allowed: true,
            hide_other_processes: true,
        })
        .unwrap();
        assert!(profile.contains("com.apple.system.opendirectoryd.libinfo"));
        assert!(profile.contains("(deny process-info*)"));
        assert!(profile.contains("(deny sysctl-read)"));
        assert!(!profile.contains("com.apple.bsd.dirhelper"));
        assert!(!profile.contains("com.apple.FSEvents"));
        assert!(!profile.contains("com.apple.system.opendirectoryd.api"));
        assert!(!profile.contains("com.apple.system.opendirectoryd.membership"));
    }

    #[test]
    fn closed_experiment_profile_omits_ordinary_etc_alias() {
        let profile = sandbox_profile(&SandboxPolicy {
            roots: vec![PathBuf::from("/tmp/workspace")],
            read_only_roots: Vec::new(),
            temporary_directory: PathBuf::from("/tmp/job"),
            network_allowed: false,
            hide_other_processes: true,
        })
        .unwrap();
        assert!(!profile.contains("(allow file-read-metadata (literal \"/etc\"))"));
        assert!(profile.contains("(allow sysctl-read (sysctl-name \"hw.ncpu\"))"));
        assert!(profile.contains("(deny process-info*)"));
    }

    #[test]
    fn network_allowed_profile_grants_keychain_agent_and_vcs_credentials() {
        let mut profile = String::new();
        append_credential_rules(
            &mut profile,
            Some(PathBuf::from("/Users/example/Library/Keychains")),
            Some(PathBuf::from("/private/var/run/com.apple.launchd.abc123")),
            Some(PathBuf::from("/Users/example")),
        )
        .unwrap();
        assert!(profile.contains("(global-name \"com.apple.securityd\")"));
        assert!(
            profile.contains("(allow file-read* (subpath \"/Users/example/Library/Keychains\"))")
        );
        assert!(profile.contains(
            "(allow file-read* file-write* (subpath \"/private/var/run/com.apple.launchd.abc123\"))"
        ));
        assert!(profile.contains(
            "(allow file-read* file-write* (subpath \"/var/run/com.apple.launchd.abc123\"))"
        ));
        assert!(
            profile.contains("(allow file-read* file-write* (subpath \"/Users/example/.ssh\"))")
        );
        assert!(profile
            .contains("(allow file-read* file-write* (subpath \"/Users/example/.config/gh\"))"));
        assert!(profile
            .contains("(allow file-read* file-write* (literal \"/Users/example/.gitconfig\"))"));
        assert!(profile.contains(
            "(allow file-read* file-write* (literal \"/Users/example/.git-credentials\"))"
        ));
    }

    #[test]
    fn network_denied_profile_omits_credential_grants() {
        let profile = sandbox_profile(&SandboxPolicy {
            roots: vec![PathBuf::from("/tmp/workspace")],
            read_only_roots: Vec::new(),
            temporary_directory: PathBuf::from("/tmp/job"),
            network_allowed: false,
            hide_other_processes: false,
        })
        .unwrap();
        assert!(profile.contains("(deny network*)"));
        assert!(!profile.contains("com.apple.securityd"));
        assert!(!profile.contains("Library/Keychains"));
        assert!(!profile.contains(".config/gh"));
        assert!(!profile.contains(".git-credentials"));
    }
}
