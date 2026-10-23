use schemars::JsonSchema;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::watch;
use uuid::Uuid;

pub const TOOL_NAME: &str = "run_privileged";
pub const REDACTION_MARKER: &str = "[REDACTED PRIVILEGE SECRET]";
const MAX_ARGUMENTS: usize = 128;
const MAX_FIELD_BYTES: usize = 4 * 1024;
const MAX_INVOCATION_BYTES: usize = 32 * 1024;
const PRIVATE_CAPTURE_BYTES: usize = 64 * 1024;
const MAX_TIMEOUT_SECS: u64 = 60 * 60;
const AUTHORIZATION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const PRIVATE_READER_JOIN_TIMEOUT: Duration = Duration::from_secs(2);
const PRIVATE_TERM_RELAY_GRACE: Duration = Duration::from_secs(2);
const PROCESS_GROUP_TERM_GRACE: Duration = Duration::from_millis(100);
const PROCESS_GROUP_KILL_GRACE: Duration = Duration::from_secs(1);
const FIXED_ASKPASS_PATH: &str = "/bin/cat";
const CLOSED_PRIVILEGED_PROGRAMS: &[&str] = &[
    "/usr/bin/id",
    "/usr/bin/whoami",
    "/usr/bin/stat",
    "/bin/ls",
    "/bin/cat",
    "/bin/chmod",
    "/usr/sbin/chown",
    "/usr/bin/chgrp",
    "/bin/mkdir",
    "/bin/rmdir",
    "/bin/rm",
    "/bin/cp",
    "/bin/mv",
    "/usr/bin/touch",
    "/bin/ln",
    "/usr/bin/install",
];

fn fixed_privilege_path() -> &'static str {
    if cfg!(target_os = "freebsd") {
        "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
    } else {
        "/usr/bin:/bin:/usr/sbin:/sbin"
    }
}

fn sudo_path_for_os(os: &str) -> Option<&'static str> {
    match os {
        "macos" | "linux" => Some("/usr/bin/sudo"),
        "freebsd" => Some("/usr/local/bin/sudo"),
        _ => None,
    }
}

fn required_sandbox_supported_on_os(os: &str) -> bool {
    os == "macos"
}

fn supported_host() -> bool {
    sudo_path_for_os(std::env::consts::OS).is_some()
}

pub struct PrivilegeSupervisor {
    disabled: AtomicBool,
}

impl PrivilegeSupervisor {
    fn new() -> Self {
        Self {
            disabled: AtomicBool::new(false),
        }
    }

    fn ensure_enabled(&self) -> Result<(), String> {
        if self.disabled.load(Ordering::Acquire) {
            Err(cleanup_latched_message())
        } else {
            Ok(())
        }
    }

    fn disable_after_cleanup_failure(&self) {
        self.disabled.store(true, Ordering::Release);
    }

    #[cfg(test)]
    fn disabled(&self) -> bool {
        self.disabled.load(Ordering::Acquire)
    }
}

impl std::fmt::Debug for PrivilegeSupervisor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrivilegeSupervisor")
            .field("disabled", &self.disabled.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
struct SupervisorHandle(Arc<PrivilegeSupervisor>);

impl SupervisorHandle {
    fn production() -> Self {
        static PROCESS: OnceLock<Arc<PrivilegeSupervisor>> = OnceLock::new();
        Self(
            PROCESS
                .get_or_init(|| Arc::new(PrivilegeSupervisor::new()))
                .clone(),
        )
    }

    #[cfg(test)]
    fn isolated() -> Self {
        Self(Arc::new(PrivilegeSupervisor::new()))
    }

    fn ensure_enabled(&self) -> Result<(), String> {
        self.0.ensure_enabled()
    }

    fn disable_after_cleanup_failure(&self) {
        self.0.disable_after_cleanup_failure();
    }
}

impl PartialEq for SupervisorHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SupervisorHandle {}

impl std::fmt::Debug for SupervisorHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SupervisorHandle([HOST ONLY])")
    }
}

fn cleanup_latched_message() -> String {
    "Privilege execution is disabled for this process because private process cleanup failed or was indeterminate; restart ca before requesting another administrator command".into()
}

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RunPrivilegedArgs {
    pub program: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPrivilegeApproval {
    pub tool_id: String,
    pub tool_args: String,
    pub invocation: PreparedPrivilegeInvocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPrivilegeSecret {
    pub interaction_id: crate::interaction::InteractionId,
    pub tool_id: String,
    pub invocation: PreparedPrivilegeInvocation,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIdentity {
    path: PathBuf,
    device: u64,
    inode: u64,
    uid: u32,
    mode: u32,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectoryIdentity {
    path: PathBuf,
    device: u64,
    inode: u64,
    uid: u32,
    mode: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PrivatePrivilegeHandle {
    owner: crate::jobs::JobOwner,
    nonce: Uuid,
}

impl std::fmt::Debug for PrivatePrivilegeHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PrivatePrivilegeHandle([HOST ONLY])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PreparedPrivilegeInvocation {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    cwd_identity: DirectoryIdentity,
    pub sandbox_mode: crate::sandbox::SandboxMode,
    pub roots: Vec<PathBuf>,
    helper: PathBuf,
    askpass: PathBuf,
    program_identity: FileIdentity,
    helper_identity: FileIdentity,
    askpass_identity: FileIdentity,
    handle: PrivatePrivilegeHandle,
    digest: String,
    requested_timeout_secs: Option<u64>,
    production_trust: bool,
    one_use: OneUseState,
    supervisor: SupervisorHandle,
}

#[derive(Clone)]
struct OneUseState(Arc<AtomicBool>);

impl OneUseState {
    fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    fn consume(&self) -> Result<(), String> {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| "The privilege authorization was already used".into())
    }
}

impl PartialEq for OneUseState {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for OneUseState {}

impl std::fmt::Debug for PreparedPrivilegeInvocation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedPrivilegeInvocation")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("cwd", &self.cwd)
            .field("sandbox_mode", &self.sandbox_mode)
            .field("roots", &self.roots)
            .field("helper", &self.helper)
            .field("askpass", &self.askpass)
            .field("digest", &self.digest)
            .field("requested_timeout_secs", &self.requested_timeout_secs)
            .field("handle", &self.handle)
            .finish()
    }
}

impl PreparedPrivilegeInvocation {
    pub fn prepare(
        args: RunPrivilegedArgs,
        workspace: &crate::workspace::WorkspaceContext,
        sandbox_mode: crate::sandbox::SandboxMode,
        owner: crate::jobs::JobOwner,
    ) -> Result<Self, String> {
        if !supported_host() {
            return Err(
                "Privilege handoff is available only in the interactive macOS, Linux, or FreeBSD TUI"
                    .into(),
            );
        }
        let supervisor = SupervisorHandle::production();
        supervisor.ensure_enabled()?;
        validate_shape(&args)?;
        let requested_timeout_secs = args.timeout_secs;
        let _ = sandbox_mode;
        let sandbox_mode = crate::sandbox::SandboxMode::Required;
        if !required_sandbox_supported_on_os(std::env::consts::OS)
            || !crate::sandbox::SandboxBackend::new(sandbox_mode).is_available()
        {
            return Err(
                "Privilege handoff requires an OS-enforced system-wide filesystem sandbox with network denial; this backend is currently available only on macOS"
                    .into(),
            );
        }
        let program = canonical_absolute(&args.program, "program")?;
        validate_closed_program(&program)?;
        let program_identity = trusted_native_identity(&program, false)?;
        let helper = PathBuf::from(
            sudo_path_for_os(std::env::consts::OS)
                .ok_or_else(|| "Privilege helper is unavailable on this host".to_string())?,
        );
        let helper_identity = trusted_native_identity(&helper, true)?;
        let askpass = PathBuf::from(FIXED_ASKPASS_PATH);
        let askpass_identity = trusted_native_identity(&askpass, false)?;
        let cwd = workspace
            .resolve(
                args.cwd
                    .as_deref()
                    .unwrap_or_else(|| workspace.cwd().to_str().unwrap_or_default()),
                crate::workspace::PathOperation::Read,
            )
            .map_err(|error| error.to_string())?;
        if !cwd.is_dir() {
            return Err("privileged cwd must be an existing workspace directory".into());
        }
        let cwd_identity = directory_identity(&cwd)?;
        Self::finish(
            program,
            args.args,
            cwd,
            cwd_identity,
            sandbox_mode,
            vec![PathBuf::from("/")],
            helper,
            askpass,
            program_identity,
            helper_identity,
            askpass_identity,
            requested_timeout_secs,
            true,
            PrivatePrivilegeHandle {
                owner,
                nonce: Uuid::new_v4(),
            },
            supervisor,
        )
    }

    #[cfg(test)]
    pub fn prepare_for_test(
        program: PathBuf,
        args: Vec<String>,
        cwd: PathBuf,
        helper: PathBuf,
        sandbox_mode: crate::sandbox::SandboxMode,
        roots: Vec<PathBuf>,
        owner: crate::jobs::JobOwner,
    ) -> Result<Self, String> {
        let cwd = cwd.canonicalize().map_err(|error| error.to_string())?;
        let roots = roots
            .into_iter()
            .map(|root| root.canonicalize().map_err(|error| error.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        validate_shape(&RunPrivilegedArgs {
            program: program.to_string_lossy().into_owned(),
            args: args.clone(),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            timeout_secs: None,
        })?;
        let program = program.canonicalize().map_err(|error| error.to_string())?;
        let helper = helper.canonicalize().map_err(|error| error.to_string())?;
        let askpass = PathBuf::from(FIXED_ASKPASS_PATH)
            .canonicalize()
            .map_err(|error| error.to_string())?;
        Self::finish(
            program.clone(),
            args,
            cwd.clone(),
            directory_identity(&cwd)?,
            sandbox_mode,
            roots,
            helper.clone(),
            askpass.clone(),
            identity(&program)?,
            identity(&helper)?,
            identity(&askpass)?,
            None,
            false,
            PrivatePrivilegeHandle {
                owner,
                nonce: Uuid::from_u128(1),
            },
            SupervisorHandle::isolated(),
        )
    }

    #[cfg(test)]
    pub fn display_fixture_for_test(
        program: &str,
        args: Vec<String>,
        cwd: &str,
        sandbox_mode: crate::sandbox::SandboxMode,
        digest: &str,
    ) -> Self {
        let identity = |path: &str| FileIdentity {
            path: PathBuf::from(path),
            device: 1,
            inode: 1,
            uid: 0,
            mode: 0o100755,
            size: 1,
            modified_seconds: 1,
            modified_nanoseconds: 0,
        };
        Self {
            program: PathBuf::from(program),
            args,
            cwd: PathBuf::from(cwd),
            cwd_identity: DirectoryIdentity {
                path: PathBuf::from(cwd),
                device: 1,
                inode: 1,
                uid: 0,
                mode: 0o040755,
            },
            sandbox_mode,
            roots: vec![PathBuf::from("/")],
            helper: PathBuf::from("/test/fake-sudo"),
            askpass: PathBuf::from(FIXED_ASKPASS_PATH),
            program_identity: identity(program),
            helper_identity: identity("/test/fake-sudo"),
            askpass_identity: identity(FIXED_ASKPASS_PATH),
            handle: PrivatePrivilegeHandle {
                owner: crate::jobs::JobOwner::process(),
                nonce: Uuid::from_u128(1),
            },
            digest: digest.into(),
            requested_timeout_secs: None,
            production_trust: false,
            one_use: OneUseState::new(),
            supervisor: SupervisorHandle::isolated(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finish(
        program: PathBuf,
        args: Vec<String>,
        cwd: PathBuf,
        cwd_identity: DirectoryIdentity,
        sandbox_mode: crate::sandbox::SandboxMode,
        roots: Vec<PathBuf>,
        helper: PathBuf,
        askpass: PathBuf,
        program_identity: FileIdentity,
        helper_identity: FileIdentity,
        askpass_identity: FileIdentity,
        requested_timeout_secs: Option<u64>,
        production_trust: bool,
        handle: PrivatePrivilegeHandle,
        supervisor: SupervisorHandle,
    ) -> Result<Self, String> {
        supervisor.ensure_enabled()?;
        let digest = invocation_digest(
            &program,
            &args,
            &cwd,
            &cwd_identity,
            sandbox_mode,
            &roots,
            &helper,
            &askpass,
            &program_identity,
            &helper_identity,
            &askpass_identity,
            requested_timeout_secs,
            handle.nonce,
        );
        Ok(Self {
            program,
            args,
            cwd,
            cwd_identity,
            sandbox_mode,
            roots,
            helper,
            askpass,
            program_identity,
            helper_identity,
            askpass_identity,
            handle,
            digest,
            requested_timeout_secs,
            production_trust,
            one_use: OneUseState::new(),
            supervisor,
        })
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn helper(&self) -> &Path {
        &self.helper
    }

    pub fn filesystem_authority_summary(&self) -> &'static str {
        if self.sandbox_mode == crate::sandbox::SandboxMode::Required
            && self.roots == [PathBuf::from("/")]
        {
            "system-wide"
        } else {
            "configured sandbox roots"
        }
    }

    pub fn network_enforcement_summary(&self) -> &'static str {
        if self.sandbox_mode == crate::sandbox::SandboxMode::Required {
            "OS-denied"
        } else {
            "not OS-enforced"
        }
    }

    pub fn display_arguments(&self) -> impl Iterator<Item = (usize, &str)> {
        self.args.iter().map(String::as_str).enumerate()
    }

    pub fn display_command(&self) -> String {
        let mut rendered = self.program.to_string_lossy().into_owned();
        for argument in &self.args {
            rendered.push(' ');
            rendered.push_str(
                &serde_json::to_string(argument).unwrap_or_else(|_| "\"[invalid]\"".into()),
            );
        }
        rendered
    }

    pub fn hook_metadata(&self) -> serde_json::Value {
        serde_json::json!({
            "tool_name": TOOL_NAME,
            "tool_input": {
                "program": self.program.to_string_lossy(),
                "args": self.args,
                "cwd": self.cwd.to_string_lossy(),
                "timeout_secs": self.requested_timeout_secs,
            },
            "privilege": {
                "invocation_digest": self.digest,
                "sandbox_mode": self.sandbox_mode.as_str(),
                "filesystem_authority": self.filesystem_authority_summary(),
                "network_allowed": false,
                "network_enforcement": self.network_enforcement_summary(),
            }
        })
    }

    pub fn matches_binding(&self, digest: &str, nonce: Uuid, owner: crate::jobs::JobOwner) -> bool {
        self.digest == digest && self.handle.nonce == nonce && self.handle.owner == owner
    }

    pub fn nonce(&self) -> Uuid {
        self.handle.nonce
    }

    pub fn revalidate(&self, owner: crate::jobs::JobOwner) -> Result<(), String> {
        self.supervisor.ensure_enabled()?;
        if self.handle.owner != owner {
            return Err("The private privilege owner changed; request approval again".into());
        }
        revalidate_bound_directory(&self.cwd)?;
        if directory_identity(&self.cwd)? != self.cwd_identity {
            return Err(
                "The approved privilege directory identity changed; request approval again".into(),
            );
        }
        for root in &self.roots {
            revalidate_bound_directory(root)?;
        }
        let current_program = if self.production_trust {
            trusted_native_identity(&self.program, false)?
        } else {
            identity(&self.program)?
        };
        let current_helper = if self.production_trust {
            trusted_native_identity(&self.helper, true)?
        } else {
            identity(&self.helper)?
        };
        let current_askpass = if self.production_trust {
            trusted_native_identity(&self.askpass, false)?
        } else {
            identity(&self.askpass)?
        };
        if current_program != self.program_identity
            || current_helper != self.helper_identity
            || current_askpass != self.askpass_identity
        {
            return Err(
                "The approved privilege executable binding changed; request approval again".into(),
            );
        }
        let digest = invocation_digest(
            &self.program,
            &self.args,
            &self.cwd,
            &self.cwd_identity,
            self.sandbox_mode,
            &self.roots,
            &self.helper,
            &self.askpass,
            &self.program_identity,
            &self.helper_identity,
            &self.askpass_identity,
            self.requested_timeout_secs,
            self.handle.nonce,
        );
        if digest != self.digest {
            return Err("The approved privilege invocation changed; request approval again".into());
        }
        Ok(())
    }

    fn effective_timeout(&self, runtime_limit: Duration) -> Duration {
        self.requested_timeout_secs
            .map(Duration::from_secs)
            .map_or(runtime_limit, |requested| requested.min(runtime_limit))
    }
}

fn revalidate_bound_directory(path: &Path) -> Result<(), String> {
    let canonical = path
        .canonicalize()
        .map_err(|_| "The approved privilege directory became unavailable".to_string())?;
    if canonical != path || !canonical.is_dir() {
        return Err(
            "The approved privilege directory binding changed; request approval again".into(),
        );
    }
    Ok(())
}

pub fn capability_available() -> bool {
    SupervisorHandle::production().ensure_enabled().is_ok()
        && required_sandbox_supported_on_os(std::env::consts::OS)
        && crate::sandbox::SandboxBackend::new(crate::sandbox::SandboxMode::Required).is_available()
        && sudo_path_for_os(std::env::consts::OS)
            .is_some_and(|path| trusted_native_identity(Path::new(path), true).is_ok())
        && trusted_native_identity(Path::new(FIXED_ASKPASS_PATH), false).is_ok()
}

fn validate_shape(args: &RunPrivilegedArgs) -> Result<(), String> {
    if args.program.is_empty() || args.program.len() > MAX_FIELD_BYTES {
        return Err("privileged program must contain 1 to 4096 bytes".into());
    }
    if args.args.len() > MAX_ARGUMENTS {
        return Err("privileged argv is limited to 128 arguments".into());
    }
    let mut total = args.program.len();
    for value in args.args.iter().chain(args.cwd.iter()) {
        if value.len() > MAX_FIELD_BYTES {
            return Err("privileged fields are limited to 4096 bytes".into());
        }
        if value.chars().any(char::is_control) {
            return Err("privileged fields must not contain control characters".into());
        }
        total = total.saturating_add(value.len());
    }
    if args.program.chars().any(char::is_control) {
        return Err("privileged fields must not contain control characters".into());
    }
    if total > MAX_INVOCATION_BYTES {
        return Err("privileged invocation is limited to 32768 bytes".into());
    }
    if args
        .timeout_secs
        .is_some_and(|timeout| !(1..=MAX_TIMEOUT_SECS).contains(&timeout))
    {
        return Err(format!(
            "privileged timeout_secs must be between 1 and {MAX_TIMEOUT_SECS}"
        ));
    }
    Ok(())
}

fn canonical_absolute(raw: &str, label: &str) -> Result<PathBuf, String> {
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(format!("privileged {label} must be an absolute path"));
    }
    path.canonicalize()
        .map_err(|_| format!("privileged {label} does not exist"))
}

fn validate_closed_program(program: &Path) -> Result<(), String> {
    let allowed = CLOSED_PRIVILEGED_PROGRAMS.iter().any(|candidate| {
        Path::new(candidate)
            .canonicalize()
            .is_ok_and(|candidate| candidate == program)
    });
    if !allowed {
        return Err(format!(
            "privileged program is outside the closed foreground utility set: {}",
            program.display()
        ));
    }
    Ok(())
}

fn trusted_native_identity(path: &Path, require_setuid: bool) -> Result<FileIdentity, String> {
    if !supported_host() {
        return Err(
            "trusted privilege executables are supported only on macOS, Linux, and FreeBSD".into(),
        );
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| "trusted privilege executable is unavailable".to_string())?;
    let identity = identity(&canonical)?;
    if identity.uid != 0 || identity.mode & 0o111 == 0 || identity.mode & 0o022 != 0 {
        return Err("privilege executable is not root-owned and immutable".into());
    }
    if require_setuid && identity.mode & 0o4000 == 0 {
        return Err("sudo helper is not setuid root".into());
    }
    for ancestor in canonical.parent().into_iter().flat_map(Path::ancestors) {
        let metadata = std::fs::symlink_metadata(ancestor)
            .map_err(|_| "privilege executable ancestry is unavailable".to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
                return Err("privilege executable ancestry is not root-owned and immutable".into());
            }
        }
    }
    // The fixed macOS sudo binary is intentionally execute-only on current
    // releases, so its root/setuid identity is the readable trust signal.
    // Linux and FreeBSD helpers are readable and must also prove ELF identity.
    if (!require_setuid || !cfg!(target_os = "macos")) && !is_native_executable(&canonical)? {
        return Err("privilege executable must be a native host binary".into());
    }
    Ok(identity)
}

fn native_magic_matches(os: &str, magic: [u8; 4]) -> bool {
    match os {
        "macos" => matches!(
            u32::from_be_bytes(magic),
            0xcafebabe | 0xbebafeca | 0xfeedface | 0xcefaedfe | 0xfeedfacf | 0xcffaedfe
        ),
        "linux" | "freebsd" => magic == *b"\x7fELF",
        _ => false,
    }
}

fn is_native_executable(path: &Path) -> Result<bool, String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)
        .map_err(|_| "privilege executable could not be inspected".to_string())?;
    let mut magic = [0_u8; 4];
    file.read_exact(&mut magic)
        .map_err(|_| "privilege executable is truncated".to_string())?;
    Ok(native_magic_matches(std::env::consts::OS, magic))
}

fn identity(path: &Path) -> Result<FileIdentity, String> {
    let canonical = path.canonicalize().map_err(|error| error.to_string())?;
    let metadata = std::fs::metadata(&canonical).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("privilege executable must be a regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(FileIdentity {
            path: canonical,
            device: metadata.dev(),
            inode: metadata.ino(),
            uid: metadata.uid(),
            mode: metadata.mode(),
            size: metadata.size(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
        })
    }
    #[cfg(not(unix))]
    {
        let _ = canonical;
        Err("privilege executable identity is unavailable".into())
    }
}

fn directory_identity(path: &Path) -> Result<DirectoryIdentity, String> {
    let canonical = path.canonicalize().map_err(|error| error.to_string())?;
    let metadata = std::fs::metadata(&canonical).map_err(|error| error.to_string())?;
    if !metadata.is_dir() {
        return Err("privilege working directory must be a directory".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(DirectoryIdentity {
            path: canonical,
            device: metadata.dev(),
            inode: metadata.ino(),
            uid: metadata.uid(),
            mode: metadata.mode(),
        })
    }
    #[cfg(not(unix))]
    {
        let _ = canonical;
        Err("privilege directory identity is unavailable".into())
    }
}

fn open_bound_working_directory(
    path: &Path,
    expected: &DirectoryIdentity,
) -> Result<std::fs::File, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let directory = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| "The approved privilege directory could not be opened safely")?;
        let metadata = directory
            .metadata()
            .map_err(|_| "The approved privilege directory could not be inspected safely")?;
        use std::os::unix::fs::MetadataExt;
        let current = DirectoryIdentity {
            path: expected.path.clone(),
            device: metadata.dev(),
            inode: metadata.ino(),
            uid: metadata.uid(),
            mode: metadata.mode(),
        };
        if current != *expected {
            return Err(
                "The approved privilege directory identity changed; request approval again".into(),
            );
        }
        Ok(directory)
    }
    #[cfg(not(unix))]
    {
        let _ = (path, expected);
        Err("Bound privilege working directories are unavailable".into())
    }
}

#[allow(clippy::too_many_arguments)]
fn invocation_digest(
    program: &Path,
    args: &[String],
    cwd: &Path,
    cwd_identity: &DirectoryIdentity,
    sandbox_mode: crate::sandbox::SandboxMode,
    roots: &[PathBuf],
    helper: &Path,
    askpass: &Path,
    program_identity: &FileIdentity,
    helper_identity: &FileIdentity,
    askpass_identity: &FileIdentity,
    requested_timeout_secs: Option<u64>,
    nonce: Uuid,
) -> String {
    fn add(hasher: &mut Sha256, bytes: &[u8]) {
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    fn add_identity(hasher: &mut Sha256, value: &FileIdentity) {
        add(hasher, value.path.to_string_lossy().as_bytes());
        for number in [
            value.device,
            value.inode,
            u64::from(value.uid),
            u64::from(value.mode),
            value.size,
            value.modified_seconds as u64,
            value.modified_nanoseconds as u64,
        ] {
            hasher.update(number.to_be_bytes());
        }
    }
    fn add_directory_identity(hasher: &mut Sha256, value: &DirectoryIdentity) {
        add(hasher, value.path.to_string_lossy().as_bytes());
        for number in [
            value.device,
            value.inode,
            u64::from(value.uid),
            u64::from(value.mode),
        ] {
            hasher.update(number.to_be_bytes());
        }
    }
    let mut hasher = Sha256::new();
    add(&mut hasher, b"ca.privilege.v1");
    add(&mut hasher, program.to_string_lossy().as_bytes());
    for argument in args {
        add(&mut hasher, argument.as_bytes());
    }
    add(&mut hasher, cwd.to_string_lossy().as_bytes());
    add_directory_identity(&mut hasher, cwd_identity);
    add(&mut hasher, sandbox_mode.as_str().as_bytes());
    for root in roots {
        add(&mut hasher, root.to_string_lossy().as_bytes());
    }
    add(&mut hasher, helper.to_string_lossy().as_bytes());
    add(&mut hasher, askpass.to_string_lossy().as_bytes());
    add_identity(&mut hasher, program_identity);
    add_identity(&mut hasher, helper_identity);
    add_identity(&mut hasher, askpass_identity);
    hasher.update(requested_timeout_secs.unwrap_or(0).to_be_bytes());
    add(&mut hasher, nonce.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(crate) struct SecretPattern {
    bytes: Mutex<Vec<u8>>,
    secret: Mutex<Option<crate::interaction::SecretHandle>>,
}

impl SecretPattern {
    #[cfg(test)]
    fn new(bytes: &[u8]) -> Self {
        Self {
            bytes: Mutex::new(bytes.to_vec()),
            secret: Mutex::new(None),
        }
    }

    fn with_secret(secret: crate::interaction::SecretHandle) -> Self {
        Self {
            bytes: Mutex::new(secret.expose_bytes().to_vec()),
            secret: Mutex::new(Some(secret)),
        }
    }

    fn take_secret(&self) -> Option<crate::interaction::SecretHandle> {
        self.secret
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
    }
}

impl std::fmt::Debug for SecretPattern {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretPattern([REDACTED])")
    }
}

impl Drop for SecretPattern {
    fn drop(&mut self) {
        self.bytes
            .get_mut()
            .unwrap_or_else(|poison| poison.into_inner())
            .fill(0);
        drop(
            self.secret
                .get_mut()
                .unwrap_or_else(|poison| poison.into_inner())
                .take(),
        );
    }
}

struct StreamingRedactor {
    pattern: Arc<SecretPattern>,
    pending: Vec<u8>,
}

impl StreamingRedactor {
    fn new(pattern: Arc<SecretPattern>) -> Self {
        Self {
            pattern,
            pending: Vec::new(),
        }
    }

    fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(bytes);
        let pattern = self
            .pattern
            .bytes
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if pattern.is_empty() {
            return std::mem::take(&mut self.pending);
        }
        let mut output = Vec::new();
        let mut consumed = 0;
        while consumed + pattern.len() <= self.pending.len() {
            if self.pending[consumed..].starts_with(pattern.as_slice()) {
                output.extend_from_slice(REDACTION_MARKER.as_bytes());
                consumed += pattern.len();
            } else {
                output.push(self.pending[consumed]);
                consumed += 1;
            }
        }
        self.pending.drain(..consumed);
        output
    }

    fn finish(mut self) -> Vec<u8> {
        let mut output = self.push(&[]);
        output.append(&mut self.pending);
        output
    }
}

impl Drop for StreamingRedactor {
    fn drop(&mut self) {
        self.pending.fill(0);
    }
}

#[derive(Default)]
struct BoundedPrivateText {
    head: Vec<u8>,
    tail: Vec<u8>,
    total: usize,
}

impl BoundedPrivateText {
    fn push(&mut self, bytes: &[u8]) {
        self.total = self.total.saturating_add(bytes.len());
        let head_capacity = PRIVATE_CAPTURE_BYTES.div_ceil(2);
        let tail_capacity = PRIVATE_CAPTURE_BYTES / 2;
        let remaining = head_capacity.saturating_sub(self.head.len());
        self.head
            .extend_from_slice(&bytes[..bytes.len().min(remaining)]);
        self.tail.extend_from_slice(bytes);
        if self.tail.len() > tail_capacity {
            self.tail.drain(..self.tail.len() - tail_capacity);
        }
    }

    fn omitted(&self) -> usize {
        self.total.saturating_sub(PRIVATE_CAPTURE_BYTES)
    }

    fn render(&self, label: &str) -> String {
        if self.total <= PRIVATE_CAPTURE_BYTES {
            let suffix = self.total.saturating_sub(self.head.len());
            let mut bytes = self.head.clone();
            if suffix > 0 {
                bytes.extend_from_slice(&self.tail[self.tail.len() - suffix..]);
            }
            return String::from_utf8_lossy(&bytes).into_owned();
        }
        format!(
            "{}\n[… {label}: {} bytes omitted …]\n{}",
            String::from_utf8_lossy(&self.head),
            self.omitted(),
            String::from_utf8_lossy(&self.tail)
        )
    }
}

struct PrivateProcessOutput {
    status: crate::jobs::JobStatus,
    stdout: BoundedPrivateText,
    stderr: BoundedPrivateText,
    elapsed: Duration,
}

struct ProcessGroupGuard {
    pid: Option<u32>,
}

struct PrivateAskpassInput {
    program: PathBuf,
    fifo: PathBuf,
    secret: crate::interaction::SecretHandle,
}

impl std::fmt::Debug for PrivateAskpassInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PrivateAskpassInput([REDACTED])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AskpassDelivery {
    Delivered,
    Unused,
}

struct ZeroizingReadBuffer([u8; 4096]);

impl Drop for ZeroizingReadBuffer {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

impl ProcessGroupGuard {
    fn disarm(&mut self) {
        self.pid = None;
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            unsafe {
                let _ = libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}

async fn read_private_stream<R>(
    mut reader: R,
    pattern: Option<Arc<SecretPattern>>,
) -> BoundedPrivateText
where
    R: AsyncRead + Unpin,
{
    let mut capture = BoundedPrivateText::default();
    let mut redactor = pattern.map(StreamingRedactor::new);
    let mut chunk = ZeroizingReadBuffer([0_u8; 4096]);
    loop {
        match reader.read(&mut chunk.0).await {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                if let Some(redactor) = redactor.as_mut() {
                    let mut redacted = redactor.push(&chunk.0[..count]);
                    capture.push(&redacted);
                    redacted.fill(0);
                } else {
                    capture.push(&chunk.0[..count]);
                }
                chunk.0.fill(0);
            }
        }
    }
    if let Some(redactor) = redactor {
        let mut redacted = redactor.finish();
        capture.push(&redacted);
        redacted.fill(0);
    }
    capture
}

async fn run_private_process(
    program: &Path,
    arguments: &[String],
    cwd: &Path,
    timeout: Duration,
    askpass: Option<PrivateAskpassInput>,
    pattern: Option<Arc<SecretPattern>>,
) -> Result<PrivateProcessOutput, String> {
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let mut cancel = CancelOnDrop(Some(cancel_tx));
    let task = tokio::spawn(run_private_process_owned(
        program.to_path_buf(),
        arguments.to_vec(),
        cwd.to_path_buf(),
        None,
        timeout,
        askpass,
        pattern,
        cancel_rx,
    ));
    let result = task
        .await
        .map_err(|_| "Private privilege process supervisor failed".to_string())?;
    cancel.disarm();
    result
}

struct CancelOnDrop(Option<watch::Sender<bool>>);

impl CancelOnDrop {
    fn disarm(&mut self) {
        self.0.take();
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(cancel) = self.0.take() {
            let _ = cancel.send(true);
        }
    }
}

async fn wait_for_cancellation(cancel: &mut watch::Receiver<bool>) {
    if *cancel.borrow() {
        return;
    }
    let _ = cancel.changed().await;
}

#[allow(clippy::too_many_arguments)]
async fn run_private_process_owned(
    program: PathBuf,
    arguments: Vec<String>,
    cwd: PathBuf,
    bound_cwd: Option<std::fs::File>,
    timeout: Duration,
    askpass: Option<PrivateAskpassInput>,
    pattern: Option<Arc<SecretPattern>>,
    mut cancel: watch::Receiver<bool>,
) -> Result<PrivateProcessOutput, String> {
    if *cancel.borrow() {
        return Err("Private privilege process cancelled".into());
    }
    let mut command = Command::new(&program);
    command
        .args(&arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .env("PATH", fixed_privilege_path())
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .kill_on_drop(true);
    if bound_cwd.is_none() {
        command.current_dir(&cwd);
    }
    if let Some(askpass) = askpass.as_ref() {
        command.env("SUDO_ASKPASS", &askpass.program);
    }
    #[cfg(unix)]
    unsafe {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        command.as_std_mut().pre_exec(move || {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if let Some(directory) = bound_cwd.as_ref() {
                if libc::fchdir(directory.as_raw_fd()) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|_| "Could not start the private privilege process".to_string())?;
    drop(command);
    let mut guard = ProcessGroupGuard { pid: child.id() };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Private privilege stdout was unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Private privilege stderr was unavailable".to_string())?;
    let stdout_task = tokio::spawn(read_private_stream(stdout, pattern.clone()));
    let stderr_task = tokio::spawn(read_private_stream(stderr, pattern));
    let (askpass_stop_tx, askpass_stop_rx) = watch::channel(false);
    let mut askpass_task = askpass.map(|askpass| {
        tokio::spawn(deliver_askpass_once(
            askpass.secret,
            askpass.fifo,
            askpass_stop_rx,
        ))
    });
    enum WaitOutcome {
        Finished(std::io::Result<std::process::ExitStatus>),
        TimedOut,
        Cancelled,
        AskpassFailed(String),
    }
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let wait = loop {
        tokio::select! {
            status = child.wait() => break WaitOutcome::Finished(status),
            _ = &mut deadline => break WaitOutcome::TimedOut,
            _ = wait_for_cancellation(&mut cancel) => break WaitOutcome::Cancelled,
            delivery = async {
                askpass_task
                    .as_mut()
                    .expect("askpass branch is guarded")
                    .await
            }, if askpass_task.is_some() => {
                askpass_task = None;
                match delivery {
                    Ok(Ok(AskpassDelivery::Delivered)) => {}
                    Ok(Ok(AskpassDelivery::Unused)) => {
                        break WaitOutcome::AskpassFailed(
                            "Administrator authentication input became stale".into(),
                        );
                    }
                    Ok(Err(error)) => break WaitOutcome::AskpassFailed(error),
                    Err(_) => {
                        break WaitOutcome::AskpassFailed(
                            "Administrator authentication input supervisor failed".into(),
                        );
                    }
                }
            }
        }
    };
    let _ = askpass_stop_tx.send(true);
    let (status, cancelled, process_error, termination_error) = match wait {
        WaitOutcome::Finished(Ok(status)) => (Some(status), false, None, None),
        WaitOutcome::Finished(Err(_)) => (
            None,
            false,
            Some("Private privilege process failed while waiting".into()),
            None,
        ),
        WaitOutcome::TimedOut => {
            let termination_error = terminate_private_child(&mut child, guard.pid).await.err();
            (None, false, None, termination_error)
        }
        WaitOutcome::Cancelled => {
            let termination_error = terminate_private_child(&mut child, guard.pid).await.err();
            (None, true, None, termination_error)
        }
        WaitOutcome::AskpassFailed(error) => {
            let termination_error = terminate_private_child(&mut child, guard.pid).await.err();
            (None, false, Some(error), termination_error)
        }
    };
    let group_cleanup = terminate_remaining_process_group(guard.pid).await;
    if group_cleanup.is_ok() {
        guard.disarm();
    }
    if let Some(mut task) = askpass_task {
        match tokio::time::timeout(PRIVATE_READER_JOIN_TIMEOUT, &mut task).await {
            Ok(Ok(Ok(AskpassDelivery::Delivered | AskpassDelivery::Unused))) => {}
            Ok(Ok(Err(error))) => return Err(error),
            Ok(Err(_)) => return Err("Administrator authentication input supervisor failed".into()),
            Err(_) => {
                task.abort();
                let _ = task.await;
                return Err("Administrator authentication input cleanup timed out".into());
            }
        }
    }
    group_cleanup?;
    let stdout = join_private_reader(stdout_task, "stdout").await?;
    let stderr = join_private_reader(stderr_task, "stderr").await?;
    if let Some(error) = termination_error {
        return Err(error);
    }
    if let Some(error) = process_error {
        return Err(error);
    }
    if cancelled {
        return Err("Private privilege process cancelled".into());
    }
    let status = if status.is_none() {
        crate::jobs::JobStatus::TimedOut
    } else if status
        .as_ref()
        .is_some_and(std::process::ExitStatus::success)
    {
        crate::jobs::JobStatus::Completed {
            exit_code: status.and_then(|status| status.code()).unwrap_or(0),
        }
    } else {
        crate::jobs::JobStatus::Failed {
            exit_code: status.and_then(|status| status.code()),
        }
    };
    Ok(PrivateProcessOutput {
        status,
        stdout,
        stderr,
        elapsed: started.elapsed(),
    })
}

async fn terminate_private_child(
    child: &mut tokio::process::Child,
    pid: Option<u32>,
) -> Result<(), String> {
    // sudo may place its monitor and command in a separate terminal session.
    // A catchable signal gives the main sudo process a bounded opportunity to
    // relay termination and reap that session before the supervisor resorts
    // to SIGKILL, which sudo cannot relay.
    signal_process_group(pid, libc::SIGTERM);
    match tokio::time::timeout(PRIVATE_TERM_RELAY_GRACE, child.wait()).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(_)) => Err("Private privilege relay cleanup became indeterminate".into()),
        Err(_) => {
            signal_process_group(pid, libc::SIGKILL);
            let _ = child.start_kill();
            let _ = tokio::time::timeout(PROCESS_GROUP_KILL_GRACE, child.wait()).await;
            Err("Private privilege descendant cleanup became indeterminate after forced termination"
                .into())
        }
    }
}

fn signal_process_group(pid: Option<u32>, signal: libc::c_int) {
    #[cfg(unix)]
    if let Some(pid) = pid.and_then(|pid| i32::try_from(pid).ok()) {
        unsafe {
            let _ = libc::kill(-pid, signal);
        }
    }
    #[cfg(not(unix))]
    let _ = (pid, signal);
}

#[cfg(unix)]
fn process_group_exists(pid: u32) -> Result<bool, String> {
    let pid = i32::try_from(pid).map_err(|_| "Private privilege process id overflowed")?;
    let result = unsafe { libc::kill(-pid, 0) };
    if result == 0 {
        return Ok(true);
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Err("Private privilege process group state became indeterminate".into()),
    }
}

async fn terminate_remaining_process_group(pid: Option<u32>) -> Result<(), String> {
    #[cfg(unix)]
    {
        let Some(pid) = pid else {
            return Ok(());
        };
        if !process_group_exists(pid)? {
            return Ok(());
        }
        signal_process_group(Some(pid), libc::SIGTERM);
        let term_deadline = Instant::now() + PROCESS_GROUP_TERM_GRACE;
        while Instant::now() < term_deadline {
            if !process_group_exists(pid)? {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        signal_process_group(Some(pid), libc::SIGKILL);
        let kill_deadline = Instant::now() + PROCESS_GROUP_KILL_GRACE;
        while Instant::now() < kill_deadline {
            if !process_group_exists(pid)? {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Err("Private privilege process group cleanup was indeterminate".into())
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Err("Private privilege process group ownership is unavailable".into())
    }
}

async fn join_private_reader(
    mut task: tokio::task::JoinHandle<BoundedPrivateText>,
    label: &str,
) -> Result<BoundedPrivateText, String> {
    match tokio::time::timeout(PRIVATE_READER_JOIN_TIMEOUT, &mut task).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(_)) => Err(format!("Private privilege {label} reader failed")),
        Err(_) => {
            task.abort();
            let _ = task.await;
            Err(format!(
                "Private privilege {label} reader cleanup timed out"
            ))
        }
    }
}

async fn deliver_askpass_once(
    mut secret: crate::interaction::SecretHandle,
    fifo: PathBuf,
    mut stop: watch::Receiver<bool>,
) -> Result<AskpassDelivery, String> {
    #[cfg(unix)]
    let result = {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        loop {
            if *stop.borrow() {
                break Ok(AskpassDelivery::Unused);
            }
            match std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
                .open(&fifo)
            {
                Ok(mut writer) => {
                    let write = writer
                        .write_all(secret.expose_bytes())
                        .and_then(|()| writer.write_all(b"\n"))
                        .and_then(|()| writer.flush());
                    drop(writer);
                    if write.is_err() {
                        break Err("Administrator authentication input became stale".into());
                    }
                    break Ok(AskpassDelivery::Delivered);
                }
                Err(error)
                    if error
                        .raw_os_error()
                        .is_some_and(|code| code == libc::ENXIO || code == libc::EINTR) =>
                {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(5)) => {}
                        _ = wait_for_cancellation(&mut stop) => {
                            break Ok(AskpassDelivery::Unused);
                        }
                    }
                }
                Err(_) => {
                    break Err("Administrator authentication input channel failed".into());
                }
            }
        }
    };
    #[cfg(not(unix))]
    let result = {
        let _ = (&fifo, &mut stop);
        Err("Administrator authentication input channel is unavailable".into())
    };
    secret.zeroize();
    let _ = std::fs::remove_file(&fifo);
    result
}

fn latch_cleanup_failure(invocation: &PreparedPrivilegeInvocation) -> String {
    invocation.supervisor.disable_after_cleanup_failure();
    cleanup_latched_message()
}

fn private_cleanup_indeterminate(error: &str) -> bool {
    error.contains("cleanup")
        || error.contains("reader failed")
        || error.contains("supervisor failed")
        || error.contains("process group state")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachedAuthorization {
    Available,
    PasswordRequired,
}

pub async fn check_cached_authorization(
    invocation: &PreparedPrivilegeInvocation,
    owner: crate::jobs::JobOwner,
) -> Result<CachedAuthorization, String> {
    invocation.revalidate(owner)?;
    let output = run_private_process(
        invocation.helper(),
        &["-n".into(), "-v".into()],
        Path::new("/"),
        AUTHORIZATION_PROBE_TIMEOUT,
        None,
        None,
    )
    .await
    .map_err(|error| {
        if private_cleanup_indeterminate(&error) {
            latch_cleanup_failure(invocation)
        } else {
            error
        }
    })?;
    invocation.revalidate(owner)?;
    match output.status {
        crate::jobs::JobStatus::Completed { exit_code: 0 } => Ok(CachedAuthorization::Available),
        crate::jobs::JobStatus::Failed { .. } => Ok(CachedAuthorization::PasswordRequired),
        _ => Err("Administrator authorization check timed out or did not complete".into()),
    }
}

pub async fn authenticate(
    invocation: &PreparedPrivilegeInvocation,
    owner: crate::jobs::JobOwner,
    secret: crate::interaction::SecretHandle,
) -> Result<Arc<SecretPattern>, String> {
    invocation.revalidate(owner)?;
    Ok(Arc::new(SecretPattern::with_secret(secret)))
}

pub async fn execute(
    invocation: &PreparedPrivilegeInvocation,
    owner: crate::jobs::JobOwner,
    timeout: Duration,
    pattern: Option<Arc<SecretPattern>>,
) -> crate::agent::registry::ToolExecutionResult {
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let mut cancel = CancelOnDrop(Some(cancel_tx));
    let owned_invocation = invocation.clone();
    let task = tokio::spawn(async move {
        execute_inner(&owned_invocation, owner, timeout, pattern, cancel_rx).await
    });
    match task.await {
        Ok(result) => {
            cancel.disarm();
            result
        }
        Err(_) => {
            crate::agent::registry::ToolExecutionResult::Failure(latch_cleanup_failure(invocation))
        }
    }
}

async fn execute_inner(
    invocation: &PreparedPrivilegeInvocation,
    owner: crate::jobs::JobOwner,
    runtime_timeout: Duration,
    pattern: Option<Arc<SecretPattern>>,
    cancel: watch::Receiver<bool>,
) -> crate::agent::registry::ToolExecutionResult {
    if let Err(error) = invocation.revalidate(owner) {
        return crate::agent::registry::ToolExecutionResult::Failure(error);
    }
    let temporary_directory = match create_private_temporary_directory() {
        Ok(directory) => directory,
        Err(_) => {
            return crate::agent::registry::ToolExecutionResult::Failure(
                "Could not create the private privilege temporary directory".into(),
            );
        }
    };
    let temporary_path = temporary_directory.path().to_path_buf();
    if let Err(error) = secure_private_directory(&temporary_path) {
        return crate::agent::registry::ToolExecutionResult::Failure(error);
    }
    let fifo = temporary_path.join("askpass");
    if pattern.is_some() {
        if let Err(error) = create_private_fifo(&fifo) {
            return crate::agent::registry::ToolExecutionResult::Failure(error);
        }
    }
    let roots = invocation.roots.clone();
    let prepared = crate::sandbox::SandboxBackend::new(invocation.sandbox_mode)
        .prepare_process_with_denied_paths(
            &invocation.program,
            &invocation.args,
            &crate::sandbox::SandboxPolicy {
                roots,
                read_only_roots: Vec::new(),
                temporary_directory: temporary_path.clone(),
                network_allowed: false,
                hide_other_processes: true,
            },
            std::slice::from_ref(&temporary_path),
        );
    let result = match prepared {
        Ok(prepared) => {
            if let Err(error) = invocation
                .revalidate(owner)
                .and_then(|_| invocation.one_use.consume())
            {
                return crate::agent::registry::ToolExecutionResult::Failure(error);
            }
            let bound_cwd =
                match open_bound_working_directory(&invocation.cwd, &invocation.cwd_identity) {
                    Ok(directory) => directory,
                    Err(error) => {
                        return crate::agent::registry::ToolExecutionResult::Failure(error);
                    }
                };
            let (mut arguments, askpass) = if let Some(pattern) = pattern.as_ref() {
                let Some(secret) = pattern.take_secret() else {
                    return crate::agent::registry::ToolExecutionResult::Failure(
                        "The masked administrator password was already consumed".into(),
                    );
                };
                (
                    vec![
                        "-A".into(),
                        "-p".into(),
                        fifo.to_string_lossy().into_owned(),
                    ],
                    Some(PrivateAskpassInput {
                        program: invocation.askpass.clone(),
                        fifo,
                        secret,
                    }),
                )
            } else {
                (vec!["-n".into()], None)
            };
            arguments.extend(["-H".into(), "--".into()]);
            arguments.push(prepared.program.to_string_lossy().into_owned());
            arguments.extend(prepared.arguments);
            let result = run_private_process_owned(
                invocation.helper.clone(),
                arguments,
                invocation.cwd.clone(),
                Some(bound_cwd),
                invocation.effective_timeout(runtime_timeout),
                askpass,
                pattern,
                cancel,
            )
            .await;
            match result {
                Err(error) if private_cleanup_indeterminate(&error) => {
                    Err(latch_cleanup_failure(invocation))
                }
                result => result,
            }
        }
        Err(error) => Err(error.to_string()),
    };
    let output = match result {
        Ok(output) => output,
        Err(error) => return crate::agent::registry::ToolExecutionResult::Failure(error),
    };
    let job = crate::jobs::JobInfo {
        id: crate::jobs::JobId::new(),
        command: invocation.display_command(),
        cwd: invocation.cwd.to_string_lossy().into_owned(),
        status: output.status,
        elapsed: output.elapsed,
        stdout: output.stdout.render("stdout"),
        stderr: output.stderr.render("stderr"),
        stdout_omitted_bytes: output.stdout.omitted(),
        stderr_omitted_bytes: output.stderr.omitted(),
        background: false,
    };
    let success = matches!(
        job.status,
        crate::jobs::JobStatus::Completed { exit_code: 0 }
    );
    crate::agent::registry::ToolExecutionResult::Command {
        model_text: job.formatted_output(),
        observation: crate::command_output::CommandObservation::from_job(&job, false),
        success,
        mutation: None,
    }
}

fn create_private_temporary_directory() -> Result<tempfile::TempDir, std::io::Error> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("ca-privilege-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    builder.tempdir_in(Path::new("/tmp"))
}

fn secure_private_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|_| {
            "Could not secure the private privilege temporary directory".to_string()
        })?;
        let metadata = std::fs::symlink_metadata(path).map_err(|_| {
            "Could not inspect the private privilege temporary directory".to_string()
        })?;
        if !metadata.is_dir() || metadata.permissions().mode() & 0o777 != 0o700 {
            return Err("The private privilege temporary directory is not mode 0700".into());
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err("Private privilege directories are unavailable on this platform".into())
    }
}

fn create_private_fifo(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::{FileTypeExt, PermissionsExt};

        let path_bytes = path.as_os_str().as_bytes();
        let path = CString::new(path_bytes)
            .map_err(|_| "The private privilege input path is invalid".to_string())?;
        if unsafe { libc::mkfifo(path.as_ptr(), 0o600) } != 0 {
            return Err("Could not create the private privilege input channel".into());
        }
        let filesystem_path = Path::new(std::ffi::OsStr::from_bytes(path.as_bytes()));
        std::fs::set_permissions(filesystem_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|_| "Could not secure the private privilege input channel".to_string())?;
        let metadata = std::fs::symlink_metadata(filesystem_path)
            .map_err(|_| "Could not inspect the private privilege input channel".to_string())?;
        if !metadata.file_type().is_fifo() || metadata.permissions().mode() & 0o777 != 0o600 {
            return Err("The private privilege input channel is not a mode 0600 FIFO".into());
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err("Private privilege input channels are unavailable on this platform".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn write_executable(path: &Path, contents: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, contents).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[cfg(unix)]
    fn fake_privilege_fixture() -> (tempfile::TempDir, PreparedPrivilegeInvocation) {
        let directory = tempfile::tempdir().unwrap();
        let helper = directory.path().join("fake-sudo");
        let target = directory.path().join("target");
        write_executable(
            &helper,
            r##"#!/bin/sh
base=${0%/*}
argv=$base/argv
log=$base/sudo-log
{
  printf 'BEGIN\n'
  for value in "$@"; do printf 'ARG=%s\n' "$value"; done
  printf 'PATH=%s\n' "$PATH"
  printf 'LC_ALL=%s\n' "$LC_ALL"
  printf 'LANG=%s\n' "$LANG"
  printf 'HOME=%s\n' "${HOME-}"
  printf 'SUDO_ASKPASS=%s\n' "${SUDO_ASKPASS-}"
  printf 'END\n'
} >> "$log"
if [ "$1" = "-n" ] && [ "$2" = "-v" ] && [ "$#" = 2 ]; then
  [ -f "$base/timestamp" ] || [ -f "$base/nopasswd" ]
  exit $?
fi
if [ "$1" = "-A" ] && [ "$2" = "-p" ]; then
  prompt=$3
  shift 3
  [ "$1" = "-H" ] && [ "$2" = "--" ] || exit 64
  shift 2
  printf '%s' "$$" > "$base/pid"
  if [ ! -f "$base/nopasswd" ] && [ ! -f "$base/timestamp" ]; then
    if [ -f "$base/slow-before-askpass" ]; then sleep 2; fi
    printf 'asked\n' >> "$base/askpass-invoked"
    [ "${SUDO_ASKPASS-}" = "/bin/cat" ] || exit 65
    password=$("$SUDO_ASKPASS" "$prompt") || exit 66
    if [ -f "$base/reject-password" ]; then exit 1; fi
    printf 'sudo-out:%s' "$password"
    printf 'sudo-err:%s' "$password" >&2
  fi
  : > "$base/timestamp"
elif [ "$1" = "-n" ] && [ "$2" = "-H" ] && [ "$3" = "--" ]; then
  if [ ! -f "$base/timestamp" ] && [ ! -f "$base/nopasswd" ]; then
    printf 'sudo: a password is required\n' >&2
    exit 1
  fi
  printf '%s' "$$" > "$base/pid"
  shift 3
else
  exit 64
fi
  printf 'executed\n' >> "$base/target-executed"
  : > "$argv"
  for value in "$@"; do printf '%s\n' "$value" >> "$argv"; done
  unset SUDO_ASKPASS
  if [ -f "$base/monitor-setsid" ]; then
    trap 'kill -TERM "$monitor_pid" 2>/dev/null; wait "$monitor_pid"; exit 143' TERM
    CA_PRIVILEGE_MONITOR_PID="$base/monitor-pid" \
      CA_PRIVILEGE_MONITOR_MARKER="$base/monitor-survived" "$@" &
    monitor_pid=$!
    wait "$monitor_pid"
    exit $?
  fi
  exec "$@"
"##,
        );
        write_executable(
            &target,
            r##"#!/bin/sh
base=${0%/*}
{
  printf 'PATH=%s\n' "$PATH"
  printf 'LC_ALL=%s\n' "$LC_ALL"
  printf 'LANG=%s\n' "$LANG"
  printf 'HOME=%s\n' "${HOME-}"
} > "$base/target-env"
if IFS= read -r ignored; then
  printf 'stdin-open\n' >&2
  exit 9
fi
case "$1" in
  --fail) exit 7 ;;
  --sleep) sleep 5 ;;
  --linger) (sleep 0.25; printf survived > "$2") & wait ;;
  --leader-exits) (sleep 0.25; printf survived > "$2") & exit 0 ;;
esac
printf 'target-ok\n'
"##,
        );
        let invocation = PreparedPrivilegeInvocation::prepare_for_test(
            target,
            vec!["space value".into(), "$(touch nope);&|".into()],
            directory.path().to_path_buf(),
            helper,
            crate::sandbox::SandboxMode::Off,
            vec![directory.path().to_path_buf()],
            crate::jobs::JobOwner::process(),
        )
        .unwrap();
        (directory, invocation)
    }

    fn secret_handle(value: &str) -> crate::interaction::SecretHandle {
        use crossterm::event::{KeyCode, KeyModifiers};
        let mut state = crate::interaction::InteractionState::host(
            crate::interaction::InteractionRequest::host_masked("secret", None),
        );
        for character in value.chars() {
            assert!(state
                .handle_key(KeyCode::Char(character), KeyModifiers::NONE)
                .is_none());
        }
        let resolution = state
            .handle_key(KeyCode::Enter, KeyModifiers::NONE)
            .unwrap();
        match resolution.outcome {
            crate::interaction::InteractionOutcome::Secret(secret) => secret,
            _ => panic!("expected secret resolution"),
        }
    }

    #[cfg(unix)]
    async fn wait_for_pid(path: &Path) -> i32 {
        for _ in 0..200 {
            if let Ok(value) = std::fs::read_to_string(path) {
                if let Ok(pid) = value.trim().parse() {
                    return pid;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("fake privilege process did not publish its pid");
    }

    #[cfg(unix)]
    async fn wait_for_process_exit(pid: i32) {
        for _ in 0..300 {
            if unsafe { libc::kill(pid, 0) } == -1 {
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::ESRCH)
                );
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("fake privilege process {pid} was not reaped");
    }

    #[cfg(unix)]
    fn assert_no_regular_file_contains(directory: &Path, secret: &[u8]) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                let bytes = std::fs::read(entry.path()).unwrap();
                assert!(
                    !bytes.windows(secret.len()).any(|window| window == secret),
                    "secret was persisted in {}",
                    entry.path().display()
                );
            }
        }
    }

    #[test]
    fn shape_is_bounded_and_closed_to_control_characters() {
        assert!(validate_shape(&RunPrivilegedArgs {
            program: "/usr/bin/id".into(),
            args: vec!["-u".into()],
            cwd: None,
            timeout_secs: None,
        })
        .is_ok());
        assert!(validate_shape(&RunPrivilegedArgs {
            program: "/usr/bin/id".into(),
            args: vec!["bad\narg".into()],
            cwd: None,
            timeout_secs: None,
        })
        .is_err());
        assert!(validate_shape(&RunPrivilegedArgs {
            program: "/usr/bin/id".into(),
            args: Vec::new(),
            cwd: None,
            timeout_secs: Some(0),
        })
        .is_err());
        assert!(validate_shape(&RunPrivilegedArgs {
            program: "/usr/bin/id".into(),
            args: Vec::new(),
            cwd: None,
            timeout_secs: Some(MAX_TIMEOUT_SECS + 1),
        })
        .is_err());
        assert!(validate_shape(&RunPrivilegedArgs {
            program: "/usr/bin/id".into(),
            args: vec!["x".into(); MAX_ARGUMENTS + 1],
            cwd: None,
            timeout_secs: None,
        })
        .is_err());
        assert!(validate_shape(&RunPrivilegedArgs {
            program: "/usr/bin/id".into(),
            args: vec!["x".repeat(MAX_FIELD_BYTES + 1)],
            cwd: None,
            timeout_secs: None,
        })
        .is_err());
        assert!(validate_shape(&RunPrivilegedArgs {
            program: "/usr/bin/id".into(),
            args: vec!["x".repeat(MAX_FIELD_BYTES); 8],
            cwd: None,
            timeout_secs: None,
        })
        .is_err());
        assert!(
            serde_json::from_value::<RunPrivilegedArgs>(serde_json::json!({
                "program": "/usr/bin/id",
                "args": [],
                "stdin": "not allowed"
            }))
            .is_err()
        );
    }

    #[test]
    fn privileged_programs_use_a_closed_non_dispatching_set() {
        for name in [
            "sh",
            "bash-5.2",
            "zsh5",
            "python3",
            "node20",
            "env",
            "xargs",
            "osascript",
            "sudo",
            "find",
            "awk",
            "make",
            "launchctl",
        ] {
            assert!(validate_closed_program(Path::new(&format!("/usr/bin/{name}"))).is_err());
        }
        let id = Path::new("/usr/bin/id").canonicalize().unwrap();
        assert!(validate_closed_program(&id).is_ok());
    }

    #[test]
    fn streaming_redaction_covers_chunk_boundaries_and_zero_length_prefixes() {
        let pattern = Arc::new(SecretPattern::new(b"correct horse"));
        let mut redactor = StreamingRedactor::new(pattern);
        let mut output = redactor.push(b"before correct ");
        output.extend(redactor.push(b"horse after"));
        output.extend(redactor.finish());
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output, format!("before {REDACTION_MARKER} after"));
        assert!(!output.contains("correct horse"));
    }

    #[cfg(unix)]
    #[test]
    fn askpass_channel_is_a_mode_0600_fifo_inside_a_mode_0700_directory() {
        use std::os::unix::fs::{FileTypeExt, PermissionsExt};

        let directory = create_private_temporary_directory().unwrap();
        let initial_metadata = std::fs::symlink_metadata(directory.path()).unwrap();
        assert_eq!(initial_metadata.permissions().mode() & 0o777, 0o700);
        secure_private_directory(directory.path()).unwrap();
        let fifo = directory.path().join("askpass");
        create_private_fifo(&fifo).unwrap();
        let directory_metadata = std::fs::symlink_metadata(directory.path()).unwrap();
        let fifo_metadata = std::fs::symlink_metadata(&fifo).unwrap();
        assert_eq!(directory_metadata.permissions().mode() & 0o777, 0o700);
        assert!(fifo_metadata.file_type().is_fifo());
        assert_eq!(fifo_metadata.permissions().mode() & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fake_sudo_reuses_os_cache_between_separately_approved_commands() {
        let (directory, first) = fake_privilege_fixture();
        let owner = crate::jobs::JobOwner::process();
        assert_eq!(
            check_cached_authorization(&first, owner).await.unwrap(),
            CachedAuthorization::PasswordRequired
        );
        let pattern = authenticate(&first, owner, secret_handle("ca-once-62e4"))
            .await
            .unwrap();
        let result = execute(&first, owner, Duration::from_secs(2), Some(pattern)).await;
        assert!(result.is_success(), "{result:?}");
        let second = PreparedPrivilegeInvocation::prepare_for_test(
            first.program.clone(),
            vec!["second command".into()],
            first.cwd.clone(),
            first.helper.clone(),
            crate::sandbox::SandboxMode::Off,
            first.roots.clone(),
            owner,
        )
        .unwrap();
        assert!(first.one_use != second.one_use);
        let cached = check_cached_authorization(&second, owner).await.unwrap();
        assert_eq!(cached, CachedAuthorization::Available);
        let result = execute(&second, owner, Duration::from_secs(2), None).await;
        assert!(result.is_success(), "{result:?}");
        let asks = std::fs::read_to_string(directory.path().join("askpass-invoked")).unwrap();
        assert_eq!(asks.lines().count(), 1);
        let targets = std::fs::read_to_string(directory.path().join("target-executed")).unwrap();
        assert_eq!(targets.lines().count(), 2);
        let log = std::fs::read_to_string(directory.path().join("sudo-log")).unwrap();
        assert!(!log.contains("ARG=-k\n"), "{log}");
        assert_no_regular_file_contains(directory.path(), b"ca-once-62e4");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fake_sudo_direct_password_flow_preserves_argv_and_closes_target_stdin() {
        let (directory, invocation) = fake_privilege_fixture();
        let owner = crate::jobs::JobOwner::process();
        assert_eq!(
            check_cached_authorization(&invocation, owner)
                .await
                .unwrap(),
            CachedAuthorization::PasswordRequired
        );
        let probe_log = std::fs::read_to_string(directory.path().join("sudo-log")).unwrap();
        assert!(probe_log.starts_with("BEGIN\nARG=-n\nARG=-v\n"));
        let pattern = authenticate(&invocation, owner, secret_handle("correct horse"))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(directory.path().join("sudo-log")).unwrap(),
            probe_log
        );
        let execution = execute(&invocation, owner, Duration::from_secs(2), Some(pattern)).await;
        assert!(execution.is_success(), "{execution:?}");
        assert!(!execution.model_text().contains("correct horse"));
        assert!(execution.model_text().contains(REDACTION_MARKER));
        assert!(execution.model_text().contains("target-ok"));
        assert!(!execution.model_text().contains("stdin-open"));
        assert!(!format!("{execution:?}").contains("correct horse"));
        let observation = execution.command_observation().unwrap();
        assert!(observation.stdout.contains(REDACTION_MARKER));
        assert!(observation.stderr.contains(REDACTION_MARKER));
        assert!(!observation.stdout.contains("correct horse"));
        assert!(!observation.stderr.contains("correct horse"));
        let persisted = serde_json::to_string(&crate::session::SessionV4::new(
            "privilege test",
            invocation.cwd.to_string_lossy(),
            "test-provider",
            "test-model",
            vec![crate::app::Message {
                role: crate::app::Role::Assistant,
                content: String::new(),
                tool_calls: Some(vec![crate::app::ToolCall {
                    id: "call-privilege".into(),
                    name: TOOL_NAME.into(),
                    arguments: serde_json::json!({
                        "program": invocation.program,
                        "args": invocation.args,
                        "cwd": invocation.cwd,
                    })
                    .to_string(),
                    status: crate::app::ToolStatus::Done,
                    output: Some(execution.model_text().into()),
                }]),
                thinking: None,
                timestamp: chrono::Utc::now(),
                response_items: Vec::new(),
            }],
        ))
        .unwrap();
        assert!(!persisted.contains("correct horse"));
        assert!(persisted.contains(REDACTION_MARKER));
        let argv = std::fs::read_to_string(directory.path().join("argv")).unwrap();
        let expected = format!(
            "{}\nspace value\n$(touch nope);&|\n",
            invocation.program.display()
        );
        assert_eq!(argv, expected);
        assert!(!directory.path().join("nope").exists());
        let repeated_pattern = authenticate(&invocation, owner, secret_handle("second-secret"))
            .await
            .unwrap();
        let repeated = execute(
            &invocation,
            owner,
            Duration::from_secs(2),
            Some(repeated_pattern),
        )
        .await;
        assert!(repeated.model_text().contains("already used"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fake_sudo_uses_a_minimal_environment_and_exact_supervisor_order() {
        let (directory, invocation) = fake_privilege_fixture();
        let owner = crate::jobs::JobOwner::process();
        assert_eq!(
            check_cached_authorization(&invocation, owner)
                .await
                .unwrap(),
            CachedAuthorization::PasswordRequired
        );
        let pattern = authenticate(&invocation, owner, secret_handle("correct horse"))
            .await
            .unwrap();
        let execution = execute(&invocation, owner, Duration::from_secs(2), Some(pattern)).await;
        assert!(execution.is_success(), "{execution:?}");

        let fixed_path = fixed_privilege_path();
        let log = std::fs::read_to_string(directory.path().join("sudo-log")).unwrap();
        let invocations = log
            .split("BEGIN\n")
            .filter(|entry| !entry.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(invocations.len(), 2, "{log}");
        assert!(invocations[0].starts_with("ARG=-n\nARG=-v\n"), "{log}");
        assert!(invocations[1].starts_with("ARG=-A\nARG=-p\nARG="), "{log}");
        assert!(invocations[1].contains("\nARG=-H\nARG=--\n"), "{log}");
        assert!(!log.contains("ARG=-k\n"), "{log}");
        assert!(!log.contains("ARG=-S\n"), "{log}");
        for (index, invocation) in invocations.into_iter().enumerate() {
            assert!(invocation.contains(&format!("PATH={fixed_path}\n")));
            assert!(invocation.contains("LC_ALL=C\n"));
            assert!(invocation.contains("LANG=C\n"));
            assert!(invocation.contains("HOME=\n"));
            if index == 1 {
                assert!(invocation.contains("SUDO_ASKPASS=/bin/cat\n"));
            } else {
                assert!(invocation.contains("SUDO_ASKPASS=\n"));
            }
        }
        let target_environment =
            std::fs::read_to_string(directory.path().join("target-env")).unwrap();
        assert_eq!(
            target_environment,
            format!("PATH={fixed_path}\nLC_ALL=C\nLANG=C\nHOME=\n")
        );
        assert!(!execution.model_text().contains("stdin-open"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn authorization_check_uses_os_cache_and_nopasswd_never_reads_secret() {
        let (directory, invocation) = fake_privilege_fixture();
        let owner = crate::jobs::JobOwner::process();
        assert_eq!(
            check_cached_authorization(&invocation, owner)
                .await
                .unwrap(),
            CachedAuthorization::PasswordRequired
        );
        assert!(!directory.path().join("askpass-invoked").exists());
        assert!(!directory.path().join("target-executed").exists());
        let log = std::fs::read_to_string(directory.path().join("sudo-log")).unwrap();
        assert_eq!(log.matches("BEGIN\n").count(), 1);
        assert!(log.starts_with("BEGIN\nARG=-n\nARG=-v\n"), "{log}");

        std::fs::write(directory.path().join("nopasswd"), "").unwrap();
        assert_eq!(
            check_cached_authorization(&invocation, owner)
                .await
                .unwrap(),
            CachedAuthorization::Available
        );
        let result = execute(&invocation, owner, Duration::from_secs(2), None).await;
        assert!(result.is_success(), "{result:?}");
        assert!(!directory.path().join("askpass-invoked").exists());
        let log = std::fs::read_to_string(directory.path().join("sudo-log")).unwrap();
        let calls = log
            .split("BEGIN\n")
            .filter(|entry| !entry.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 3, "{log}");
        assert!(calls[2].starts_with("ARG=-n\nARG=-H\nARG=--\n"), "{log}");
        assert!(
            calls.iter().all(|call| call.contains("SUDO_ASKPASS=\n")),
            "{log}"
        );
        assert!(!log.contains("ARG=-k\n"), "{log}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn expired_os_cache_needs_authentication_without_running_the_target() {
        let (directory, first) = fake_privilege_fixture();
        let owner = crate::jobs::JobOwner::process();
        std::fs::write(directory.path().join("timestamp"), "").unwrap();
        assert_eq!(
            check_cached_authorization(&first, owner).await.unwrap(),
            CachedAuthorization::Available
        );
        std::fs::remove_file(directory.path().join("timestamp")).unwrap();
        let expired = execute(&first, owner, Duration::from_secs(2), None).await;
        assert!(!expired.is_success());
        assert!(expired.model_text().contains("a password is required"));
        assert!(!directory.path().join("target-executed").exists());
        assert!(!directory.path().join("askpass-invoked").exists());

        let retry = PreparedPrivilegeInvocation::prepare_for_test(
            first.program.clone(),
            first.args.clone(),
            first.cwd.clone(),
            first.helper.clone(),
            crate::sandbox::SandboxMode::Off,
            first.roots.clone(),
            owner,
        )
        .unwrap();
        assert_eq!(
            check_cached_authorization(&retry, owner).await.unwrap(),
            CachedAuthorization::PasswordRequired
        );
        let pattern = authenticate(&retry, owner, secret_handle("ca-renewed-9ee7"))
            .await
            .unwrap();
        let result = execute(&retry, owner, Duration::from_secs(2), Some(pattern)).await;
        assert!(result.is_success(), "{result:?}");
        assert_eq!(
            std::fs::read_to_string(directory.path().join("askpass-invoked"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert_eq!(
            std::fs::read_to_string(directory.path().join("target-executed"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert_no_regular_file_contains(directory.path(), b"ca-renewed-9ee7");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fake_sudo_rejects_wrong_password_and_preserves_timeout_cleanup() {
        let (auth_directory, auth_invocation) = fake_privilege_fixture();
        let owner = crate::jobs::JobOwner::process();
        std::fs::write(auth_directory.path().join("reject-password"), "").unwrap();
        let pattern = authenticate(&auth_invocation, owner, secret_handle("ca-rejected-e11a"))
            .await
            .unwrap();
        let rejected = execute(
            &auth_invocation,
            owner,
            Duration::from_secs(2),
            Some(pattern),
        )
        .await;
        assert!(!rejected.is_success());
        assert!(!rejected.model_text().contains("ca-rejected-e11a"));
        let auth_log = std::fs::read_to_string(auth_directory.path().join("sudo-log")).unwrap();
        assert_eq!(auth_log.matches("BEGIN\n").count(), 1, "{auth_log}");
        assert!(!auth_log.contains("ARG=-k\n"), "{auth_log}");
        assert!(!auth_directory.path().join("target-executed").exists());
        assert!(!auth_directory.path().join("timestamp").exists());

        let (timeout_directory, timeout_base) = fake_privilege_fixture();
        let timeout = PreparedPrivilegeInvocation::prepare_for_test(
            timeout_base.program.clone(),
            vec!["--sleep".into()],
            timeout_base.cwd.clone(),
            timeout_base.helper.clone(),
            crate::sandbox::SandboxMode::Off,
            timeout_base.roots.clone(),
            owner,
        )
        .unwrap();
        let pattern = authenticate(&timeout, owner, secret_handle("ca-timeout-7e31"))
            .await
            .unwrap();
        let execution = execute(&timeout, owner, Duration::from_secs(1), Some(pattern)).await;
        assert!(!execution.is_success());
        assert!(execution.model_text().contains("timed out"));
        let timeout_log =
            std::fs::read_to_string(timeout_directory.path().join("sudo-log")).unwrap();
        assert!(
            timeout_log.contains("ARG=-A\nARG=-p\n") && timeout_log.contains("ARG=-H\nARG=--\n"),
            "{timeout_log}\n{execution:?}"
        );
        assert!(!timeout_log.contains("ARG=-k\n"), "{timeout_log}");
        let pid = wait_for_pid(&timeout_directory.path().join("pid")).await;
        wait_for_process_exit(pid).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn abandoned_masked_secret_runs_no_sudo_process() {
        let (directory, invocation) = fake_privilege_fixture();
        let pattern = authenticate(
            &invocation,
            crate::jobs::JobOwner::process(),
            secret_handle("correct horse"),
        )
        .await
        .unwrap();
        drop(pattern);
        tokio::task::yield_now().await;
        assert!(!directory.path().join("sudo-log").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancelling_direct_handoff_reaps_the_group_without_invalidating_os_cache() {
        let owner = crate::jobs::JobOwner::process();
        let (cancel_directory, cancel_invocation) = fake_privilege_fixture();
        let cancel_invocation = PreparedPrivilegeInvocation::prepare_for_test(
            cancel_invocation.program.clone(),
            vec!["--sleep".into()],
            cancel_invocation.cwd.clone(),
            cancel_invocation.helper.clone(),
            crate::sandbox::SandboxMode::Off,
            cancel_invocation.roots.clone(),
            owner,
        )
        .unwrap();
        let pattern = authenticate(&cancel_invocation, owner, secret_handle("ca-cancel-194a"))
            .await
            .unwrap();
        let task = tokio::spawn(async move {
            execute(
                &cancel_invocation,
                owner,
                Duration::from_secs(5),
                Some(pattern),
            )
            .await
        });
        let pid = wait_for_pid(&cancel_directory.path().join("pid")).await;
        task.abort();
        let _ = task.await;
        wait_for_process_exit(pid).await;
        let cancel_log = std::fs::read_to_string(cancel_directory.path().join("sudo-log")).unwrap();
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            -1,
            "cancelled direct sudo child was not reaped"
        );
        assert!(!cancel_log.contains("ARG=-k\n"), "{cancel_log}");
        assert!(!cancel_log.contains("ca-cancel-194a"));
    }

    #[cfg(unix)]
    #[test]
    fn fake_sudo_monitor_setsid_child() {
        let Some(pid_path) = std::env::var_os("CA_PRIVILEGE_MONITOR_PID") else {
            return;
        };
        let marker = std::env::var_os("CA_PRIVILEGE_MONITOR_MARKER")
            .expect("monitor fixture provides a survival marker");
        assert_ne!(
            unsafe { libc::setsid() },
            -1,
            "monitor fixture setsid failed"
        );
        std::fs::write(pid_path, std::process::id().to_string()).unwrap();
        std::thread::sleep(Duration::from_secs(1));
        std::fs::write(marker, "survived").unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancellation_relays_term_to_a_sudo_monitor_in_a_separate_session() {
        let owner = crate::jobs::JobOwner::process();
        let (directory, base) = fake_privilege_fixture();
        std::fs::write(directory.path().join("monitor-setsid"), "").unwrap();
        let invocation = PreparedPrivilegeInvocation::prepare_for_test(
            std::env::current_exe().unwrap(),
            vec![
                "--exact".into(),
                "privilege::tests::fake_sudo_monitor_setsid_child".into(),
                "--nocapture".into(),
            ],
            base.cwd.clone(),
            base.helper.clone(),
            crate::sandbox::SandboxMode::Off,
            base.roots.clone(),
            owner,
        )
        .unwrap();
        let pattern = authenticate(&invocation, owner, secret_handle("ca-monitor-72ab"))
            .await
            .unwrap();
        let task = tokio::spawn(async move {
            execute(&invocation, owner, Duration::from_secs(5), Some(pattern)).await
        });
        let pid = wait_for_pid(&directory.path().join("monitor-pid")).await;
        task.abort();
        let _ = task.await;
        wait_for_process_exit(pid).await;
        let log = std::fs::read_to_string(directory.path().join("sudo-log")).unwrap();
        tokio::time::sleep(Duration::from_millis(1100)).await;

        assert!(
            !directory.path().join("monitor-survived").exists(),
            "sudo monitor command survived catchable cancellation"
        );
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            -1,
            "sudo monitor command was not reaped"
        );
        assert!(!log.contains("ARG=-k\n"), "{log}");
        assert!(!log.contains("ca-monitor-72ab"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fake_sudo_command_failure_timeout_stale_binding_and_reaping_fail_closed() {
        let (_directory, mut invocation) = fake_privilege_fixture();
        let owner = crate::jobs::JobOwner::process();
        let pattern = authenticate(&invocation, owner, secret_handle("correct horse"))
            .await
            .unwrap();

        invocation.args = vec!["--fail".into()];
        let stale = execute(&invocation, owner, Duration::from_secs(1), Some(pattern)).await;
        assert!(!stale.is_success());
        assert!(stale.model_text().contains("invocation changed"));

        let (_failure_directory, failure_base) = fake_privilege_fixture();
        let failure = PreparedPrivilegeInvocation::prepare_for_test(
            failure_base.program.clone(),
            vec!["--fail".into()],
            failure_base.cwd.clone(),
            failure_base.helper.clone(),
            crate::sandbox::SandboxMode::Off,
            failure_base.roots.clone(),
            owner,
        )
        .unwrap();
        let pattern = authenticate(&failure, owner, secret_handle("ca-failure-d403"))
            .await
            .unwrap();
        let failure = execute(&failure, owner, Duration::from_secs(1), Some(pattern)).await;
        assert!(!failure.is_success());
        assert!(failure.model_text().contains("exit code: 7"));

        let (_timeout_directory, timeout_base) = fake_privilege_fixture();
        let timeout = PreparedPrivilegeInvocation::prepare_for_test(
            timeout_base.program.clone(),
            vec!["--sleep".into()],
            timeout_base.cwd.clone(),
            timeout_base.helper.clone(),
            crate::sandbox::SandboxMode::Off,
            timeout_base.roots.clone(),
            owner,
        )
        .unwrap();
        let pattern = authenticate(&timeout, owner, secret_handle("ca-timeout-0e91"))
            .await
            .unwrap();
        let timed_out = execute(&timeout, owner, Duration::from_millis(500), Some(pattern)).await;
        assert!(!timed_out.is_success());
        assert!(timed_out.model_text().contains("timed out"));

        let (reap_directory, reap_base) = fake_privilege_fixture();
        let marker = reap_directory.path().join("survived");
        let reap = PreparedPrivilegeInvocation::prepare_for_test(
            reap_base.program.clone(),
            vec!["--linger".into(), marker.to_string_lossy().into_owned()],
            reap_base.cwd.clone(),
            reap_base.helper.clone(),
            crate::sandbox::SandboxMode::Off,
            reap_base.roots.clone(),
            owner,
        )
        .unwrap();
        let pattern = authenticate(&reap, owner, secret_handle("ca-reap-f4c0"))
            .await
            .unwrap();
        let task = tokio::spawn(async move {
            execute(&reap, owner, Duration::from_secs(2), Some(pattern)).await
        });
        let pid = wait_for_pid(&reap_directory.path().join("pid")).await;
        task.abort();
        let _ = task.await;
        wait_for_process_exit(pid).await;
        let reap_log = std::fs::read_to_string(reap_directory.path().join("sudo-log")).unwrap();
        tokio::time::sleep(Duration::from_millis(350)).await;
        assert!(!marker.exists(), "cancelled private process group survived");
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            -1,
            "private child was not reaped"
        );
        assert!(!reap_log.contains("ARG=-k\n"), "{reap_log}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn successful_leader_cannot_leave_a_background_descendant() {
        let (directory, base) = fake_privilege_fixture();
        let owner = crate::jobs::JobOwner::process();
        let marker = directory.path().join("leader-survived");
        let invocation = PreparedPrivilegeInvocation::prepare_for_test(
            base.program.clone(),
            vec![
                "--leader-exits".into(),
                marker.to_string_lossy().into_owned(),
            ],
            base.cwd.clone(),
            base.helper.clone(),
            crate::sandbox::SandboxMode::Off,
            base.roots.clone(),
            owner,
        )
        .unwrap();
        let pattern = authenticate(&invocation, owner, secret_handle("ca-leader-e94b"))
            .await
            .unwrap();
        let result = execute(&invocation, owner, Duration::from_secs(2), Some(pattern)).await;
        assert!(result.is_success(), "{result:?}");
        tokio::time::sleep(Duration::from_millis(350)).await;
        assert!(
            !marker.exists(),
            "successful leader left a descendant behind"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn indeterminate_process_cleanup_latches_the_shared_supervisor() {
        let (_directory, invocation) = fake_privilege_fixture();
        let owner = crate::jobs::JobOwner::process();
        let message = latch_cleanup_failure(&invocation);
        assert!(message.contains("private process cleanup"));
        assert!(!message.contains("sudo -k"));
        assert!(invocation.supervisor.0.disabled());
        assert_eq!(invocation.revalidate(owner).unwrap_err(), message);
        assert_eq!(
            check_cached_authorization(&invocation, owner)
                .await
                .unwrap_err(),
            message
        );
    }

    #[cfg(unix)]
    #[test]
    fn per_call_timeout_is_validated_bound_and_caps_the_runtime_limit() {
        let (_directory, invocation) = fake_privilege_fixture();
        let mut timed = invocation.clone();
        timed.requested_timeout_secs = Some(1);
        timed.digest = invocation_digest(
            &timed.program,
            &timed.args,
            &timed.cwd,
            &timed.cwd_identity,
            timed.sandbox_mode,
            &timed.roots,
            &timed.helper,
            &timed.askpass,
            &timed.program_identity,
            &timed.helper_identity,
            &timed.askpass_identity,
            timed.requested_timeout_secs,
            timed.handle.nonce,
        );

        assert_ne!(invocation.digest(), timed.digest());
        assert_eq!(
            timed.effective_timeout(Duration::from_secs(30)),
            Duration::from_secs(1)
        );
        assert_eq!(
            invocation.effective_timeout(Duration::from_secs(30)),
            Duration::from_secs(30)
        );
        assert!(timed.revalidate(crate::jobs::JobOwner::process()).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn one_use_nonce_and_executable_identity_are_revalidated() {
        let (_directory, invocation) = fake_privilege_fixture();
        let owner = crate::jobs::JobOwner::process();
        assert!(invocation.matches_binding(invocation.digest(), invocation.nonce(), owner));
        assert!(!invocation.matches_binding(invocation.digest(), Uuid::new_v4(), owner));
        assert!(!invocation.matches_binding("wrong", invocation.nonce(), owner));
        assert!(!invocation.matches_binding(
            invocation.digest(),
            invocation.nonce(),
            crate::jobs::JobOwner::new(None),
        ));
        assert!(invocation
            .revalidate(crate::jobs::JobOwner::new(None))
            .is_err());

        let mut rebound = invocation.clone();
        rebound.sandbox_mode = crate::sandbox::SandboxMode::PolicyOnly;
        assert!(rebound.revalidate(owner).is_err());

        std::fs::write(&invocation.program, "changed").unwrap();
        assert!(invocation.revalidate(owner).is_err());

        let (_directory, invocation) = fake_privilege_fixture();
        std::fs::write(invocation.helper(), "changed").unwrap();
        assert!(invocation.revalidate(owner).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn cwd_identity_rejects_rename_and_replacement() {
        use std::os::unix::fs::MetadataExt;

        let (directory, base) = fake_privilege_fixture();
        let approved_cwd = directory.path().join("approved-cwd");
        std::fs::create_dir(&approved_cwd).unwrap();
        let invocation = PreparedPrivilegeInvocation::prepare_for_test(
            base.program.clone(),
            base.args.clone(),
            approved_cwd.clone(),
            base.helper.clone(),
            crate::sandbox::SandboxMode::Off,
            base.roots.clone(),
            crate::jobs::JobOwner::process(),
        )
        .unwrap();
        let opened = open_bound_working_directory(&approved_cwd, &invocation.cwd_identity).unwrap();
        assert_eq!(
            opened.metadata().unwrap().ino(),
            invocation.cwd_identity.inode
        );

        let moved = directory.path().join("moved-cwd");
        std::fs::rename(&approved_cwd, &moved).unwrap();
        std::fs::create_dir(&approved_cwd).unwrap();
        assert!(invocation
            .revalidate(crate::jobs::JobOwner::process())
            .unwrap_err()
            .contains("directory identity changed"));
        assert!(open_bound_working_directory(&approved_cwd, &invocation.cwd_identity).is_err());
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
    #[test]
    fn production_probe_matches_fixed_helper_and_root_secured_id_is_available() {
        let helper = sudo_path_for_os(std::env::consts::OS).unwrap();
        let expected = required_sandbox_supported_on_os(std::env::consts::OS)
            && crate::sandbox::SandboxBackend::new(crate::sandbox::SandboxMode::Required)
                .is_available()
            && trusted_native_identity(Path::new(helper), true).is_ok()
            && trusted_native_identity(Path::new(FIXED_ASKPASS_PATH), false).is_ok();
        assert_eq!(capability_available(), expected);
        assert!(trusted_native_identity(Path::new("/usr/bin/id"), false).is_ok());
        let shell = Path::new("/bin/sh").canonicalize().unwrap();
        assert!(validate_closed_program(&shell).is_err());
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    #[test]
    fn mandatory_sandbox_refuses_before_unix_privilege_preparation() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(directory.path()).unwrap();
        let error = PreparedPrivilegeInvocation::prepare(
            RunPrivilegedArgs {
                program: "/usr/bin/id".into(),
                args: vec!["-u".into()],
                cwd: None,
                timeout_secs: None,
            },
            &workspace,
            crate::sandbox::SandboxMode::Required,
            crate::jobs::JobOwner::process(),
        )
        .unwrap_err();
        assert!(error.contains("network denial"));
    }

    #[test]
    fn supported_unix_privilege_platform_contract_is_fixed() {
        assert_eq!(sudo_path_for_os("macos"), Some("/usr/bin/sudo"));
        assert_eq!(sudo_path_for_os("linux"), Some("/usr/bin/sudo"));
        assert_eq!(sudo_path_for_os("freebsd"), Some("/usr/local/bin/sudo"));
        assert_eq!(sudo_path_for_os("windows"), None);

        assert!(native_magic_matches("macos", [0xcf, 0xfa, 0xed, 0xfe]));
        assert!(native_magic_matches("linux", *b"\x7fELF"));
        assert!(native_magic_matches("freebsd", *b"\x7fELF"));
        assert!(!native_magic_matches("linux", [0xcf, 0xfa, 0xed, 0xfe]));

        assert!(required_sandbox_supported_on_os("macos"));
        assert!(!required_sandbox_supported_on_os("linux"));
        assert!(!required_sandbox_supported_on_os("freebsd"));
    }
}
