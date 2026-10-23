use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(not(unix))]
use std::fs::OpenOptions;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

use crate::cancellation::{ActiveChildProcessGroup, CancellationToken};
use crate::config::{default_guard_ignored_paths, RoleConfig};
use crate::guard::{compile_globs, expand_allowed_file_patterns, normalize_glob_pattern};
use crate::paths::Paths;

const ROLE_RUN_SCHEMA: &str = "ca.role_run.v1";
const PREFLIGHT_SCHEMA: &str = "ca.role_run_preflight.v1";
const HEADLESS_SCHEMA: &str = "ca.headless.v2";
const MAX_STDOUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 1024 * 1024;
const MAX_JSONL_LINE_BYTES: usize = 1024 * 1024;
const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_PROMPT_BYTES: usize = 256 * 1024;
const MAX_SCOPE_PATTERN_BYTES: usize = 1024;
const MAX_GRANTS: usize = 64;
const MAX_PREFLIGHT_BYTES: usize = 1024 * 1024;
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(30);
const TERMINATION_GRACE: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const READ_ONLY_TOOLS: [&str; 6] = [
    "read_file",
    "list_dir",
    "search_files",
    "glob",
    "read_todos",
    "blame",
];
static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub(crate) struct CaProcessConfig<'a> {
    pub(crate) bin: &'a Path,
    pub(crate) profile: &'a str,
    pub(crate) route_provider: &'a str,
}

#[derive(Debug, Clone)]
pub(crate) enum CaRoleAuthority<'a> {
    ReadOnly,
    Worker { allowed_files: &'a [String] },
}

pub(crate) struct CaRunRequest<'a> {
    pub(crate) role_name: &'a str,
    pub(crate) role: &'a RoleConfig,
    pub(crate) task_id: Option<&'a str>,
    pub(crate) prompt: &'a str,
    pub(crate) schema_path: &'a Path,
    pub(crate) authority: CaRoleAuthority<'a>,
}

pub(crate) type CaCancellationToken = CancellationToken;

pub(crate) trait CaRunner {
    fn run_ca(
        &self,
        paths: &Paths,
        config: CaProcessConfig<'_>,
        request: CaRunRequest<'_>,
        cancellation: &CaCancellationToken,
    ) -> Result<String>;

    fn preflight(&self, config: CaProcessConfig<'_>) -> Result<CaPreflightReceipt>;
}

pub(crate) struct RealCaRunner;

impl CaRunner for RealCaRunner {
    fn run_ca(
        &self,
        paths: &Paths,
        config: CaProcessConfig<'_>,
        request: CaRunRequest<'_>,
        cancellation: &CaCancellationToken,
    ) -> Result<String> {
        run_ca_process(paths, config, request, cancellation)
    }

    fn preflight(&self, config: CaProcessConfig<'_>) -> Result<CaPreflightReceipt> {
        run_ca_preflight(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaPreflightReceipt {
    pub(crate) schema: String,
    pub(crate) ready: bool,
    pub(crate) profile: String,
    pub(crate) sandbox_required: bool,
    #[serde(default)]
    pub(crate) routes: Vec<CaPreflightRoute>,
    #[serde(default)]
    pub(crate) issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaPreflightRoute {
    pub(crate) provider: String,
    #[serde(default)]
    pub(crate) models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaRoleRequest {
    schema: &'static str,
    request_id: String,
    provider: String,
    model: String,
    reasoning_effort: String,
    workspace: String,
    prompt: String,
    output_schema: String,
    tools: Vec<String>,
    grants: Vec<CaGrant>,
    #[serde(skip_serializing_if = "Option::is_none")]
    learning_bounds: Option<CaLearningBounds>,
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
struct CaLearningBounds {
    input_bytes: usize,
    output_tokens: u32,
}
#[derive(Debug, Clone, Default)]
pub(crate) struct CaPhysicalUsage {
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) requests: u64,
}
#[derive(Debug)]
pub(crate) struct CaLearningOutput {
    pub(crate) content: String,
    pub(crate) usage: Option<CaPhysicalUsage>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CaGrant {
    Tool { tool: String },
    Path { tool: String, path_pattern: String },
}

pub(crate) fn build_role_request(
    paths: &Paths,
    config: CaProcessConfig<'_>,
    request: &CaRunRequest<'_>,
) -> Result<CaRoleRequest> {
    validate_process_config(config.clone())?;
    if request.prompt.len() > MAX_PROMPT_BYTES {
        bail!("ca role prompt exceeds the 256 KiB limit");
    }
    let workspace = fs::canonicalize(&paths.root)
        .with_context(|| format!("failed to canonicalize workspace {}", paths.root.display()))?;
    let schema_path = canonical_regular_schema(&workspace, request.schema_path)?;
    let (tools, grants) = build_authority(&workspace, &request.authority)?;
    let request_id = safe_request_id(request.role_name, request.task_id);
    Ok(CaRoleRequest {
        schema: ROLE_RUN_SCHEMA,
        request_id,
        provider: config.route_provider.to_string(),
        model: request.role.model.clone(),
        reasoning_effort: request.role.effort.clone(),
        workspace: path_string(&workspace)?,
        prompt: request.prompt.to_string(),
        output_schema: path_string(&schema_path)?,
        tools,
        grants,
        learning_bounds: None,
    })
}

fn validate_process_config(config: CaProcessConfig<'_>) -> Result<()> {
    if !config.bin.is_absolute() {
        bail!("ca executable path must be absolute");
    }
    if config.profile.is_empty() || matches!(config.profile, "default" | "bare") {
        bail!("ca role-run requires a nonreserved named profile");
    }
    if !is_safe_identifier(config.profile) {
        bail!("ca profile must contain only safe ASCII identifier characters");
    }
    if config.route_provider.trim().is_empty() || !is_safe_identifier(config.route_provider) {
        bail!("ca route provider must be a safe nonempty identifier");
    }
    Ok(())
}

fn canonical_regular_schema(workspace: &Path, schema_path: &Path) -> Result<PathBuf> {
    let metadata = fs::symlink_metadata(schema_path)
        .with_context(|| format!("failed to inspect output schema {}", schema_path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("ca output schema must be a regular, non-symlink file");
    }
    let canonical = fs::canonicalize(schema_path).with_context(|| {
        format!(
            "failed to canonicalize output schema {}",
            schema_path.display()
        )
    })?;
    if !canonical.starts_with(workspace) || canonical == workspace {
        bail!("ca output schema must be contained inside the target workspace");
    }
    Ok(canonical)
}

fn build_authority(
    workspace: &Path,
    authority: &CaRoleAuthority<'_>,
) -> Result<(Vec<String>, Vec<CaGrant>)> {
    let mut tools = READ_ONLY_TOOLS
        .iter()
        .map(|tool| (*tool).to_string())
        .collect::<Vec<_>>();
    let mut grants = READ_ONLY_TOOLS
        .iter()
        .map(|tool| CaGrant::Tool {
            tool: (*tool).to_string(),
        })
        .collect::<Vec<_>>();
    if let CaRoleAuthority::Worker { allowed_files } = authority {
        for raw in *allowed_files {
            validate_relative_scope(raw)?;
        }
        let expanded = expand_allowed_file_patterns(allowed_files);
        if expanded.is_empty() {
            bail!("ca Worker requires at least one allowed file pattern");
        }
        for pattern in &expanded {
            // Alias expansion and slash normalization can reveal escapes that
            // were not path components on the host platform in the raw text.
            validate_relative_scope(pattern)?;
        }
        validate_worker_write_scope(&expanded)?;
        validate_existing_worker_scope(workspace, &expanded)?;
        tools.extend(["apply_patch".to_string(), "write_file".to_string()]);
        for pattern in expanded {
            let absolute = workspace.join(&pattern);
            let absolute = path_string(&absolute)?;
            grants.push(CaGrant::Path {
                tool: "apply_patch".to_string(),
                path_pattern: absolute.clone(),
            });
            grants.push(CaGrant::Path {
                tool: "write_file".to_string(),
                path_pattern: absolute,
            });
        }
    }
    if grants.len() > MAX_GRANTS {
        bail!(
            "ca role-run authority needs {} grants, exceeding the {}-grant limit; narrow the task scope",
            grants.len(),
            MAX_GRANTS
        );
    }
    Ok((tools, grants))
}

fn validate_relative_scope(pattern: &str) -> Result<()> {
    if pattern.trim().is_empty() {
        bail!("ca Worker allowed file patterns must not be empty");
    }
    if pattern.len() > MAX_SCOPE_PATTERN_BYTES {
        bail!("ca Worker allowed file patterns must be at most {MAX_SCOPE_PATTERN_BYTES} bytes");
    }
    let path = Path::new(pattern);
    if path.is_absolute()
        || pattern.starts_with('/')
        || pattern.starts_with('\\')
        || has_windows_drive_prefix(pattern)
    {
        bail!("ca Worker allowed file patterns must be relative: {pattern:?}");
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("ca Worker allowed file pattern escapes the workspace: {pattern:?}");
    }
    Ok(())
}

fn has_windows_drive_prefix(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn validate_worker_write_scope(patterns: &[String]) -> Result<()> {
    let ignored_roots = default_guard_ignored_paths()
        .into_iter()
        .map(|pattern| normalize_glob_pattern(&pattern))
        .filter_map(|pattern| pattern.split('/').next().map(str::to_string))
        .collect::<Vec<_>>();
    for pattern in patterns {
        let normalized = normalize_glob_pattern(pattern);
        let top = normalized.split('/').next().unwrap_or_default();
        if has_glob_meta(top) {
            bail!(
                "ca Worker write scope must start with a literal safe repository path segment: {pattern:?}"
            );
        }
        if [".git", ".orchestrator", "oracles", "reference"]
            .iter()
            .any(|protected| top.eq_ignore_ascii_case(protected))
        {
            bail!(
                "ca Worker write scope may not select protected Git, runtime, oracle, or reference trees: {pattern:?}"
            );
        }
        if ignored_roots
            .iter()
            .any(|ignored| top.eq_ignore_ascii_case(ignored))
        {
            bail!(
                "ca Worker write scope overlaps a built-in guard-ignored generated/runtime root: {pattern:?}; choose a scope the post-run guard observes"
            );
        }
    }

    let matcher = compile_globs(patterns).context("invalid ca Worker allowed file pattern")?;
    const PROTECTED_PROBES: [&str; 9] = [
        ".orchestrator/state.json",
        ".orchestrator/imports/protected.json",
        ".orchestrator/runs/protected.log",
        "oracles/PROTECTED",
        "oracles/nested/PROTECTED.rs",
        "oracles/nested/PROTECTED.json",
        "reference/PROTECTED",
        "reference/nested/PROTECTED.rs",
        "reference/nested/PROTECTED.json",
    ];
    if let Some(protected) = PROTECTED_PROBES
        .iter()
        .find(|protected| matcher.is_match(protected))
    {
        bail!("ca Worker write scope reaches protected path {protected:?}; narrow the task scope");
    }
    Ok(())
}

fn has_glob_meta(component: &str) -> bool {
    component
        .bytes()
        .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b']' | b'{' | b'}'))
}

fn validate_existing_worker_scope(workspace: &Path, patterns: &[String]) -> Result<()> {
    let workspace_metadata = match fs::symlink_metadata(workspace) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // `build_authority` has pure unit coverage with synthetic roots;
            // production always arrives through `build_role_request`, which
            // canonicalizes an existing workspace before this boundary.
            return Ok(());
        }
        Err(error) => return Err(error).context("inspect ca Worker workspace"),
    };
    if workspace_metadata.file_type().is_symlink() || !workspace_metadata.is_dir() {
        bail!("ca Worker workspace must be a real, non-symlink directory");
    }
    let canonical_workspace = fs::canonicalize(workspace)
        .with_context(|| format!("canonicalize ca Worker workspace {}", workspace.display()))?;

    for pattern in patterns {
        validate_existing_scope_pattern(&canonical_workspace, pattern)?;
    }
    Ok(())
}

fn validate_existing_scope_pattern(workspace: &Path, pattern: &str) -> Result<()> {
    let normalized = normalize_glob_pattern(pattern);
    let components = normalized.split('/').collect::<Vec<_>>();
    let first_glob = components
        .iter()
        .position(|component| has_glob_meta(component));
    let literal_count = first_glob.unwrap_or(components.len());
    let mut current = workspace.to_path_buf();

    for component in components.iter().take(literal_count) {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    bail!(
                        "ca Worker write scope has an existing symlink prefix at {}; symlinked write grants are forbidden",
                        current.display()
                    );
                }
                reject_multi_link_worker_file(&current, &metadata)?;
                let canonical = fs::canonicalize(&current).with_context(|| {
                    format!("canonicalize ca Worker scope prefix {}", current.display())
                })?;
                if !canonical.starts_with(workspace) {
                    bail!(
                        "ca Worker write scope escapes the canonical workspace through {}",
                        current.display()
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("inspect ca Worker scope prefix {}", current.display())
                })
            }
        }
    }

    if first_glob.is_none() {
        return Ok(());
    }
    let literal_root = components.iter().take(literal_count).fold(
        workspace.to_path_buf(),
        |mut path, component| {
            path.push(component);
            path
        },
    );
    let metadata = match fs::symlink_metadata(&literal_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "inspect ca Worker literal scope root {}",
                    literal_root.display()
                )
            })
        }
    };
    if !metadata.is_dir() {
        return Ok(());
    }
    for entry in WalkDir::new(&literal_root).follow_links(false) {
        let entry = entry.with_context(|| {
            format!(
                "inspect existing ca Worker scope below {}",
                literal_root.display()
            )
        })?;
        if entry.file_type().is_symlink() {
            bail!(
                "ca Worker write scope contains an existing symlink at {}; symlinked write grants are forbidden",
                entry.path().display()
            );
        }
        let metadata = fs::symlink_metadata(entry.path()).with_context(|| {
            format!(
                "inspect existing ca Worker scope entry {}",
                entry.path().display()
            )
        })?;
        reject_multi_link_worker_file(entry.path(), &metadata)?;
    }
    Ok(())
}

#[cfg(unix)]
fn reject_multi_link_worker_file(path: &Path, metadata: &fs::Metadata) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    if metadata.is_file() && metadata.nlink() != 1 {
        bail!(
            "ca Worker write scope contains a multi-link regular file at {}; hardlinked write targets are forbidden",
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_multi_link_worker_file(_path: &Path, _metadata: &fs::Metadata) -> Result<()> {
    Ok(())
}

fn run_ca_process(
    paths: &Paths,
    config: CaProcessConfig<'_>,
    request: CaRunRequest<'_>,
    cancellation: &CaCancellationToken,
) -> Result<String> {
    run_ca_process_inner(paths, config, request, cancellation, false).map(|value| value.content)
}

pub(crate) fn run_ca_learning(
    paths: &Paths,
    config: CaProcessConfig<'_>,
    request: CaRunRequest<'_>,
    cancellation: &CaCancellationToken,
) -> Result<CaLearningOutput> {
    run_ca_process_inner(paths, config, request, cancellation, true)
}
fn run_ca_process_inner(
    paths: &Paths,
    config: CaProcessConfig<'_>,
    request: CaRunRequest<'_>,
    cancellation: &CaCancellationToken,
    learning: bool,
) -> Result<CaLearningOutput> {
    let mut machine_request = build_role_request(paths, config.clone(), &request)?;
    if learning {
        machine_request.learning_bounds = Some(CaLearningBounds {
            input_bytes: 131_072,
            output_tokens: 8_192,
        });
    }
    let encoded = serde_json::to_vec(&machine_request)?;
    if encoded.len() > MAX_REQUEST_BYTES {
        bail!("ca role request exceeds the 1 MiB limit");
    }
    if cancellation.is_cancelled() {
        bail!("ca {} was cancelled before launch", request.role_name);
    }
    let artifact_id = machine_request.request_id.clone();
    let mut artifacts = ProcessArtifacts::reserve(paths, &artifact_id)?;

    let mut command = Command::new(config.bin);
    command
        .arg("role-run")
        .arg("--profile")
        .arg(config.profile)
        .current_dir(&paths.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_process_group(&mut command);
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to launch ca role-run via {}", config.bin.display()))?;
    let child_id = child.id();
    let mut active_child = match ActiveChildProcessGroup::register(paths, child_id) {
        Ok(registration) => registration,
        Err(error) => {
            terminate_process_group(&mut child, child_id, TERMINATION_GRACE);
            return Err(error).context("publish ca child process group for MCP supervision");
        }
    };
    let stdout = child
        .stdout
        .take()
        .context("ca child stdout was unavailable")?;
    let stderr = child
        .stderr
        .take()
        .context("ca child stderr was unavailable")?;
    let stdout_reader = thread::spawn(move || read_bounded(stdout, MAX_STDOUT_BYTES));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, MAX_STDERR_BYTES));

    let mut stdin = child
        .stdin
        .take()
        .context("ca child stdin was unavailable")?;
    let stdin_writer = thread::spawn(move || stdin.write_all(&encoded));

    let started = Instant::now();
    let timeout = Duration::from_secs(request.role.timeout_seconds);
    let status = loop {
        if cancellation.is_cancelled() {
            terminate_process_group(&mut child, child_id, TERMINATION_GRACE);
            let _ = join_reader(stdin_writer, "stdin writer");
            let stdout = join_reader(stdout_reader, "stdout")??;
            let stderr = join_reader(stderr_reader, "stderr")??;
            artifacts.persist(&stdout, &stderr)?;
            bail!("ca {} was cancelled", request.role_name);
        }
        if started.elapsed() >= timeout {
            terminate_process_group(&mut child, child_id, TERMINATION_GRACE);
            let _ = join_reader(stdin_writer, "stdin writer");
            let stdout = join_reader(stdout_reader, "stdout")??;
            let stderr = join_reader(stderr_reader, "stderr")??;
            artifacts.persist(&stdout, &stderr)?;
            bail!(
                "ca {} timed out after {} seconds",
                request.role_name,
                request.role.timeout_seconds
            );
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        thread::sleep(POLL_INTERVAL);
    };

    // The direct child can exit while a descendant in its process group still
    // holds an inherited stdout or stderr pipe open. Tear down the complete
    // group before joining the I/O threads so a successful direct-child exit
    // cannot leave this runner blocked forever waiting for EOF.
    terminate_process_group(&mut child, child_id, TERMINATION_GRACE);
    active_child.clear()?;
    let write_result = join_reader(stdin_writer, "stdin writer")?;
    let stdout = join_reader(stdout_reader, "stdout")??;
    let stderr = join_reader(stderr_reader, "stderr")??;
    artifacts.persist(&stdout, &stderr)?;
    if let Err(error) = write_result {
        bail!("failed to write ca role request: {error}");
    }
    if stdout.overflowed {
        bail!("ca stdout exceeded the {}-byte limit", MAX_STDOUT_BYTES);
    }
    if stderr.overflowed {
        bail!("ca stderr exceeded the {}-byte limit", MAX_STDERR_BYTES);
    }
    if !status.success() {
        let summary = sanitize_stderr(&stderr.bytes);
        bail!(
            "ca {} exited with {}: {}",
            request.role_name,
            status
                .code()
                .map_or_else(|| "signal".to_string(), |code| code.to_string()),
            truncate(&summary, 2_048)
        );
    }
    let structured = parse_ca_jsonl(&stdout.bytes)?;
    let usage = parse_physical_usage(&stdout.bytes)?;
    Ok(CaLearningOutput {
        content: serde_json::to_string(&structured)
            .context("failed to serialize ca structured output")?,
        usage,
    })
}

fn run_ca_preflight(config: CaProcessConfig<'_>) -> Result<CaPreflightReceipt> {
    run_ca_preflight_with_timeout(config, PREFLIGHT_TIMEOUT)
}

fn run_ca_preflight_with_timeout(
    config: CaProcessConfig<'_>,
    timeout: Duration,
) -> Result<CaPreflightReceipt> {
    validate_process_config(config.clone())?;
    let mut command = Command::new(config.bin);
    command
        .arg("role-run")
        .arg("--profile")
        .arg(config.profile)
        .arg("--preflight")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_process_group(&mut command);
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to launch ca preflight via {}", config.bin.display()))?;
    let child_id = child.id();
    let stdout = child
        .stdout
        .take()
        .context("ca preflight stdout was unavailable")?;
    let stderr = child
        .stderr
        .take()
        .context("ca preflight stderr was unavailable")?;
    let stdout_reader = thread::spawn(move || read_bounded(stdout, MAX_PREFLIGHT_BYTES));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, MAX_PREFLIGHT_BYTES));
    let started = Instant::now();
    let status = loop {
        if started.elapsed() >= timeout {
            terminate_process_group(&mut child, child_id, TERMINATION_GRACE);
            let _ = join_reader(stdout_reader, "preflight stdout");
            let _ = join_reader(stderr_reader, "preflight stderr");
            bail!(
                "ca preflight timed out after {} seconds",
                timeout.as_secs_f64()
            );
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        thread::sleep(POLL_INTERVAL);
    };
    // A conforming preflight has no descendants. Clean up any inherited
    // process-group members before joining the bounded readers so a stray
    // descendant cannot hold stdout or stderr open indefinitely.
    terminate_process_group(&mut child, child_id, TERMINATION_GRACE);
    let stdout = join_reader(stdout_reader, "preflight stdout")??;
    let stderr = join_reader(stderr_reader, "preflight stderr")??;
    if stdout.overflowed || stderr.overflowed {
        bail!("ca preflight output exceeded the 1 MiB limit");
    }
    if !status.success() {
        bail!(
            "ca preflight exited with {}: {}",
            status
                .code()
                .map_or_else(|| "signal".to_string(), |code| code.to_string()),
            truncate(&sanitize_stderr(&stderr.bytes), 2_048)
        );
    }
    let mut receipt: CaPreflightReceipt =
        serde_json::from_slice(&stdout.bytes).context("ca preflight returned invalid JSON")?;
    if receipt.schema != PREFLIGHT_SCHEMA {
        bail!("ca preflight returned an unsupported schema");
    }
    if receipt.profile != config.profile {
        bail!("ca preflight returned a different profile than requested");
    }
    if !receipt.sandbox_required {
        bail!("ca preflight reports that required sandboxing is disabled");
    }
    let route_ready = receipt
        .routes
        .iter()
        .any(|route| route.provider == config.route_provider && !route.models.is_empty());
    if receipt.ready && !route_ready {
        bail!("ca preflight did not report the configured provider route");
    }
    if !receipt.issues.is_empty() {
        receipt.issues = vec![format!(
            "{} redacted ca preflight issue(s)",
            receipt.issues.len()
        )];
    }
    Ok(receipt)
}

#[derive(Debug)]
struct BoundedRead {
    bytes: Vec<u8>,
    overflowed: bool,
}

fn read_bounded(mut reader: impl Read, limit: usize) -> std::io::Result<BoundedRead> {
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    reader
        .by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    let overflowed = bytes.len() > limit;
    bytes.truncate(limit);
    Ok(BoundedRead { bytes, overflowed })
}

fn join_reader<T>(handle: thread::JoinHandle<T>, name: &str) -> Result<T> {
    handle
        .join()
        .map_err(|_| anyhow::anyhow!("ca {name} reader thread panicked"))
}

fn parse_ca_jsonl(bytes: &[u8]) -> Result<Value> {
    let text = std::str::from_utf8(bytes).context("ca stdout was not valid UTF-8")?;
    let mut expected_sequence = 0u64;
    let mut run_id: Option<String> = None;
    let mut structured: Option<(u64, Value)> = None;
    let mut completion_sequence: Option<u64> = None;
    let mut last_sequence: Option<u64> = None;
    for (index, line) in text.lines().enumerate() {
        if line.len() > MAX_JSONL_LINE_BYTES {
            bail!("ca JSONL line {} exceeded the 1 MiB limit", index + 1);
        }
        if line.trim().is_empty() {
            bail!("ca stdout contained an empty JSONL record");
        }
        let envelope: CaEnvelope = serde_json::from_str(line)
            .with_context(|| format!("ca stdout line {} was not a valid envelope", index + 1))?;
        if envelope.schema != HEADLESS_SCHEMA {
            bail!("ca returned an unsupported headless schema");
        }
        if envelope.sequence != expected_sequence {
            bail!(
                "ca JSONL sequence was not monotonic: expected {}, received {}",
                expected_sequence,
                envelope.sequence
            );
        }
        expected_sequence = expected_sequence.saturating_add(1);
        last_sequence = Some(envelope.sequence);
        match &run_id {
            None if !envelope.run_id.is_empty() => run_id = Some(envelope.run_id.clone()),
            Some(existing) if existing == &envelope.run_id => {}
            _ => bail!("ca JSONL run_id changed or was empty"),
        }
        let kind = envelope
            .event
            .get("type")
            .and_then(Value::as_str)
            .context("ca event did not contain a string type")?;
        match kind {
            "failure" => {
                bail!("ca reported a failure event; inspect the redacted ca artifacts");
            }
            "approval_required" => {
                bail!("ca required approval; no post-role action was performed");
            }
            "structured_output" => {
                if structured.is_some() {
                    bail!("ca emitted structured_output more than once");
                }
                let value = envelope
                    .event
                    .get("value")
                    .cloned()
                    .context("ca structured_output event omitted value")?;
                if !value.is_object() {
                    bail!("ca structured_output value must be an object");
                }
                structured = Some((envelope.sequence, value));
            }
            "completion" if completion_sequence.replace(envelope.sequence).is_some() => {
                bail!("ca emitted completion more than once");
            }
            "completion" => {}
            _ => {}
        }
    }
    let (structured_sequence, value) = structured.context("ca did not emit structured_output")?;
    let completion_sequence = completion_sequence.context("ca did not emit completion")?;
    if completion_sequence <= structured_sequence {
        bail!("ca completion must follow structured_output");
    }
    if Some(completion_sequence) != last_sequence {
        bail!("ca completion must be the final JSONL event");
    }
    Ok(value)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaEnvelope {
    schema: String,
    sequence: u64,
    run_id: String,
    event: Value,
}

fn safe_request_id(role_name: &str, task_id: Option<&str>) -> String {
    let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let role = sanitize_identifier(role_name);
    let task = sanitize_identifier(task_id.unwrap_or("project"));
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let epoch_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    // Keep the collision-resistant process/time/sequence suffix intact. Long
    // role or task labels are truncated only from the descriptive prefix, so
    // they cannot erase the portion that makes artifact names unique.
    let suffix = format!(
        "-{:x}-{:032x}-{:016x}",
        std::process::id(),
        epoch_nanos,
        sequence
    );
    let prefix = format!("{timestamp}-{role}-{task}");
    let prefix_budget = 128usize.saturating_sub(suffix.len());
    let mut prefix = prefix.chars().take(prefix_budget).collect::<String>();
    while prefix
        .chars()
        .last()
        .is_some_and(|ch| matches!(ch, '-' | '_' | '.'))
    {
        prefix.pop();
    }
    if prefix.is_empty() {
        prefix.push_str("run");
    }
    format!("{prefix}{suffix}")
}

fn sanitize_identifier(value: &str) -> String {
    let value = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    if value.is_empty() {
        "role".to_string()
    } else {
        value
    }
}

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn path_string(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_string)
        .context("ca role-run paths must be valid UTF-8")
}

#[derive(Debug)]
struct ProcessArtifacts {
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    stdout: File,
    stderr: File,
}

impl ProcessArtifacts {
    fn reserve(paths: &Paths, artifact_id: &str) -> Result<Self> {
        if !is_safe_identifier(artifact_id) {
            bail!("ca artifact ID was not a safe bounded identifier");
        }
        let stdout_name = format!("{artifact_id}.ca.stdout.jsonl");
        let stderr_name = format!("{artifact_id}.ca.stderr.log");
        let stdout_path = paths.runs.join(&stdout_name);
        let stderr_path = paths.runs.join(&stderr_name);
        let directory = open_secure_runs_directory(paths)?;
        let stdout =
            create_new_artifact(&directory, &stdout_name, &stdout_path).with_context(|| {
                format!(
                    "reserve unique ca stdout artifact {}",
                    stdout_path.display()
                )
            })?;
        let stderr = match create_new_artifact(&directory, &stderr_name, &stderr_path) {
            Ok(file) => file,
            Err(error) => {
                drop(stdout);
                let _ = remove_artifact(&directory, &stdout_name, &stdout_path);
                return Err(error).with_context(|| {
                    format!(
                        "reserve unique ca stderr artifact {}",
                        stderr_path.display()
                    )
                });
            }
        };
        directory
            .sync_all()
            .context("sync ca artifact directory after reservation")?;
        Ok(Self {
            stdout_path,
            stderr_path,
            stdout,
            stderr,
        })
    }

    fn persist(&mut self, stdout: &BoundedRead, stderr: &BoundedRead) -> Result<()> {
        write_reserved_artifact(
            &mut self.stdout,
            &self.stdout_path,
            &sanitize_stdout_artifact(&stdout.bytes),
        )?;
        write_reserved_artifact(
            &mut self.stderr,
            &self.stderr_path,
            sanitize_stderr(&stderr.bytes).as_bytes(),
        )
    }
}

fn write_reserved_artifact(file: &mut File, path: &Path, bytes: &[u8]) -> Result<()> {
    file.write_all(bytes)
        .with_context(|| format!("write reserved ca artifact {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("sync reserved ca artifact {}", path.display()))
}

#[cfg(unix)]
fn open_secure_runs_directory(paths: &Paths) -> Result<File> {
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::FromRawFd;

    fn open_dir_at(parent: Option<&File>, name: &CStr) -> std::io::Result<File> {
        use std::os::unix::io::AsRawFd;
        let fd = unsafe {
            match parent {
                Some(parent) => libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                ),
                None => libc::open(
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                ),
            }
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    let root = CString::new(paths.root.as_os_str().as_bytes())
        .context("ca workspace path contained an interior NUL")?;
    let root = open_dir_at(None, &root)
        .with_context(|| format!("open canonical ca workspace {}", paths.root.display()))?;
    let orchestrator = CString::new(".orchestrator").expect("literal has no NUL");
    let orchestrator = open_dir_at(Some(&root), &orchestrator).with_context(|| {
        format!(
            "open real orchestrator runtime directory {} without following symlinks",
            paths.orch.display()
        )
    })?;
    let runs = CString::new("runs").expect("literal has no NUL");
    match open_dir_at(Some(&orchestrator), &runs) {
        Ok(directory) => Ok(directory),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            use std::os::unix::io::AsRawFd;
            let result = unsafe { libc::mkdirat(orchestrator.as_raw_fd(), runs.as_ptr(), 0o700) };
            if result != 0 {
                let create_error = std::io::Error::last_os_error();
                if create_error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(create_error).with_context(|| {
                        format!(
                            "create contained ca artifact directory {}",
                            paths.runs.display()
                        )
                    });
                }
            }
            orchestrator
                .sync_all()
                .context("sync orchestrator directory after creating ca runs directory")?;
            open_dir_at(Some(&orchestrator), &runs).with_context(|| {
                format!(
                    "open real ca artifact directory {} without following symlinks",
                    paths.runs.display()
                )
            })
        }
        Err(error) => Err(error).with_context(|| {
            format!(
                "open real ca artifact directory {} without following symlinks",
                paths.runs.display()
            )
        }),
    }
}

#[cfg(not(unix))]
fn open_secure_runs_directory(paths: &Paths) -> Result<File> {
    let root = fs::canonicalize(&paths.root)?;
    let orchestrator = fs::symlink_metadata(&paths.orch)
        .with_context(|| format!("inspect orchestrator directory {}", paths.orch.display()))?;
    if orchestrator.file_type().is_symlink() || !orchestrator.is_dir() {
        bail!("orchestrator runtime root must be a real directory");
    }
    match fs::symlink_metadata(&paths.runs) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            bail!("ca artifact directory must be a real directory")
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&paths.runs)?;
        }
        Err(error) => return Err(error.into()),
    }
    let canonical_orchestrator = fs::canonicalize(&paths.orch)?;
    let canonical_runs = fs::canonicalize(&paths.runs)?;
    if canonical_orchestrator != root.join(".orchestrator")
        || canonical_runs != canonical_orchestrator.join("runs")
    {
        bail!("ca artifact directory escapes the canonical workspace");
    }
    File::open(&canonical_runs).context("open ca artifact directory")
}

#[cfg(unix)]
fn create_new_artifact(directory: &File, name: &str, _path: &Path) -> std::io::Result<File> {
    use std::ffi::CString;
    use std::os::unix::io::{AsRawFd, FromRawFd};

    let name = CString::new(name)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "artifact NUL"))?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(not(unix))]
fn create_new_artifact(_directory: &File, _name: &str, path: &Path) -> std::io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

#[cfg(unix)]
fn remove_artifact(directory: &File, name: &str, _path: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::io::AsRawFd;

    let name = CString::new(name)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "artifact NUL"))?;
    let result = unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn remove_artifact(_directory: &File, _name: &str, path: &Path) -> std::io::Result<()> {
    fs::remove_file(path)
}

fn sanitize_stdout_artifact(bytes: &[u8]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return br#"{"schema":"ca.artifact.redacted.v1","reason":"invalid_utf8"}
"#
        .to_vec();
    };
    let mut output = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let envelope = match serde_json::from_str::<Value>(line) {
            Ok(envelope) => envelope,
            Err(_) => {
                let placeholder = serde_json::json!({
                    "schema": "ca.artifact.redacted.v1",
                    "line": index + 1,
                    "reason": "malformed_jsonl"
                });
                output.extend_from_slice(
                    serde_json::to_string(&placeholder)
                        .expect("redacted ca placeholder is serializable")
                        .as_bytes(),
                );
                output.push(b'\n');
                continue;
            }
        };
        let event_type = envelope
            .get("event")
            .and_then(|event| event.get("type"))
            .and_then(Value::as_str)
            .and_then(safe_artifact_event_type)
            .unwrap_or("unknown");
        let envelope = serde_json::json!({
            "schema": "ca.artifact.redacted.v1",
            "source_schema_valid": envelope.get("schema").and_then(Value::as_str) == Some(HEADLESS_SCHEMA),
            "sequence": envelope.get("sequence").and_then(Value::as_u64),
            "run_id_present": envelope.get("run_id").and_then(Value::as_str).is_some_and(|id| !id.is_empty()),
            "event_type": event_type,
            "payload_redacted": true
        });
        output.extend_from_slice(
            serde_json::to_string(&envelope)
                .expect("parsed ca JSONL envelope is serializable")
                .as_bytes(),
        );
        output.push(b'\n');
    }
    output
}

fn safe_artifact_event_type(event_type: &str) -> Option<&'static str> {
    match event_type {
        "run_start" => Some("run_start"),
        "turn_start" => Some("turn_start"),
        "assistant_output" => Some("assistant_output"),
        "reasoning_output" => Some("reasoning_output"),
        "tool_start" => Some("tool_start"),
        "tool_finish" => Some("tool_finish"),
        "usage" => Some("usage"),
        "approval_required" => Some("approval_required"),
        "main_completion" => Some("main_completion"),
        "check_start" => Some("check_start"),
        "check_report" => Some("check_report"),
        "verification_failed" => Some("verification_failed"),
        "structured_output" => Some("structured_output"),
        "completion" => Some("completion"),
        "failure" => Some("failure"),
        _ => None,
    }
}

fn sanitize_stderr(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let line_count = bytes.iter().filter(|byte| **byte == b'\n').count()
        + usize::from(bytes.last() != Some(&b'\n'));
    format!(
        "[redacted ca diagnostic: {} byte(s), {} line(s)]",
        bytes.len(),
        line_count
    )
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

fn terminate_process_group(child: &mut std::process::Child, child_id: u32, grace: Duration) {
    #[cfg(unix)]
    signal_process_group(child_id, libc::SIGTERM);
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    let mut child_reaped = false;
    let started = Instant::now();
    while started.elapsed() < grace {
        if !child_reaped && matches!(child.try_wait(), Ok(Some(_))) {
            child_reaped = true;
        }
        #[cfg(unix)]
        if child_reaped && !process_group_exists(child_id) {
            return;
        }
        #[cfg(not(unix))]
        if child_reaped {
            return;
        }
        thread::sleep(POLL_INTERVAL);
    }
    #[cfg(unix)]
    signal_process_group(child_id, libc::SIGKILL);
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    if !child_reaped {
        let _ = child.wait();
    }

    #[cfg(unix)]
    {
        let cleanup_started = Instant::now();
        while cleanup_started.elapsed() < grace && process_group_exists(child_id) {
            thread::sleep(POLL_INTERVAL);
        }
    }
}

#[cfg(unix)]
fn signal_process_group(child_id: u32, signal: i32) {
    unsafe {
        libc::kill(-(child_id as i32), signal);
    }
}

#[cfg(unix)]
fn process_group_exists(child_id: u32) -> bool {
    let result = unsafe { libc::kill(-(child_id as i32), 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RoleProvider;
    use std::collections::BTreeSet;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-team-ca-runner-{name}-{nonce}"))
    }

    fn role() -> RoleConfig {
        RoleConfig {
            provider: RoleProvider::Codex,
            model: "gpt-test".to_string(),
            effort: "high".to_string(),
            sandbox: "read-only".to_string(),
            approval_policy: "never".to_string(),
            web_search: "disabled".to_string(),
            timeout_seconds: 5,
        }
    }

    #[cfg(unix)]
    fn make_hanging_fake_ca(root: &Path) -> Result<PathBuf> {
        use std::os::unix::fs::PermissionsExt;

        let fake = root.join("fake-ca-hanging");
        let child_pid = root.join("child.pid");
        let grandchild_pid = root.join("grandchild.pid");
        fs::write(
            &fake,
            format!(
                "#!/bin/sh\n\
                 printf '%s\\n' \"$$\" > '{}'\n\
                 sh -c 'trap \"\" TERM; while :; do sleep 1; done' &\n\
                 grandchild=$!\n\
                 printf '%s\\n' \"$grandchild\" > '{}'\n\
                 trap 'exit 0' TERM\n\
                 while :; do sleep 1; done\n",
                child_pid.display(),
                grandchild_pid.display(),
            ),
        )?;
        let mut permissions = fs::metadata(&fake)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake, permissions)?;
        Ok(fake)
    }

    #[cfg(unix)]
    fn wait_for_pid(path: &Path) -> Result<u32> {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            if let Ok(text) = fs::read_to_string(path) {
                if let Ok(pid) = text.trim().parse::<u32>() {
                    return Ok(pid);
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        bail!(
            "timed out waiting for fake process pid at {}",
            path.display()
        )
    }

    #[cfg(unix)]
    fn process_exists(pid: u32) -> bool {
        let result = unsafe { libc::kill(pid as i32, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }

    #[cfg(unix)]
    fn assert_process_exits(pid: u32) {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            if !process_exists(pid) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        unsafe {
            libc::kill(pid as i32, libc::SIGKILL);
        }
        panic!("fake ca descendant {pid} survived runner cleanup");
    }

    #[cfg(unix)]
    fn role_run_request<'a>(role: &'a RoleConfig, schema: &'a Path) -> CaRunRequest<'a> {
        CaRunRequest {
            role_name: "worker",
            role,
            task_id: Some("INT-148"),
            prompt: "wait for cancellation",
            schema_path: schema,
            authority: CaRoleAuthority::ReadOnly,
        }
    }

    #[cfg(unix)]
    fn shell_quote(path: &Path) -> Result<String> {
        let value = path
            .to_str()
            .context("real ca compatibility paths must be valid UTF-8")?;
        Ok(format!("'{}'", value.replace('\'', "'\"'\"'")))
    }

    #[cfg(unix)]
    fn install_hermetic_ca_wrapper(root: &Path, home: &Path, ca_bin: &Path) -> Result<PathBuf> {
        use std::os::unix::fs::PermissionsExt;

        let wrapper = root.join("hermetic-ca");
        let script = format!(
            r#"#!/bin/sh
exec /usr/bin/env -i \
  HOME={} \
  PATH=/usr/bin:/bin \
  ROLE_RUN_REAL_COMPAT_KEY=loopback-test-credential \
  NO_PROXY=127.0.0.1,localhost \
  no_proxy=127.0.0.1,localhost \
  GIT_CONFIG_NOSYSTEM=1 \
  GIT_CONFIG_GLOBAL=/dev/null \
  {} "$@"
"#,
            shell_quote(home)?,
            shell_quote(ca_bin)?,
        );
        fs::write(&wrapper, script)?;
        let mut permissions = fs::metadata(&wrapper)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wrapper, permissions)?;
        Ok(wrapper)
    }

    #[cfg(unix)]
    fn install_real_ca_compat_profile(
        home: &Path,
        profile: &str,
        base_url: &str,
    ) -> Result<PathBuf> {
        let directory = home.join(".config/ca/profiles");
        fs::create_dir_all(&directory)?;
        let path = directory.join(format!("{profile}.toml"));
        fs::write(
            &path,
            format!(
                r#"approval_rules = []

[agent]
max_turns = 3
approval_mode = "allowlist"
command_timeout_secs = 2
sandbox_mode = "required"

[defaults]
provider = "fake"
model = "fake-model"

[providers.fake]
base_url = "{base_url}"
api_key = "${{ROLE_RUN_REAL_COMPAT_KEY}}"
models = ["fake-model"]

[model_profiles."fake/fake-model"]
supports_tools = true
reasoning_efforts = ["high"]
default_reasoning_effort = "high"
reasoning_request_format = "reasoning_effort"
"#,
            ),
        )?;
        Ok(path)
    }

    #[cfg(unix)]
    fn compatibility_tool_sse(name: &str, id: &str, arguments: Value) -> String {
        let delta = serde_json::json!({
            "choices": [{
                "delta": {"tool_calls": [{
                    "index": 0,
                    "id": id,
                    "function": {"name": name, "arguments": arguments.to_string()}
                }]},
                "finish_reason": null
            }]
        });
        let finish = serde_json::json!({
            "choices": [{"delta": {}, "finish_reason": "tool_calls"}]
        });
        format!("data: {delta}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
    }

    #[cfg(unix)]
    fn start_real_ca_loopback_provider(
        responses: Vec<String>,
    ) -> Result<(String, thread::JoinHandle<Result<Vec<serde_json::Value>>>)> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let deadline = Instant::now() + Duration::from_secs(10);
                let (mut stream, _) = loop {
                    match listener.accept() {
                        Ok(connection) => break connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if Instant::now() >= deadline {
                                bail!("timed out waiting for real ca loopback provider request");
                            }
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => return Err(error.into()),
                    }
                };
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                let mut request = Vec::new();
                let mut chunk = [0_u8; 4096];
                let header_end = loop {
                    let count = stream.read(&mut chunk)?;
                    if count == 0 {
                        bail!("real ca provider request ended before its headers");
                    }
                    request.extend_from_slice(&chunk[..count]);
                    if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        break index + 4;
                    }
                };
                let headers = std::str::from_utf8(&request[..header_end])?;
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>())
                    })
                    .transpose()?
                    .unwrap_or(0);
                while request.len() < header_end + content_length {
                    let count = stream.read(&mut chunk)?;
                    if count == 0 {
                        bail!("real ca provider request ended before its body");
                    }
                    request.extend_from_slice(&chunk[..count]);
                }
                requests.push(serde_json::from_slice(
                    &request[header_end..header_end + content_length],
                )?);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.len(),
                    response
                )?;
                stream.flush()?;
            }
            Ok(requests)
        });
        Ok((format!("http://{address}/v1"), handle))
    }

    #[cfg(unix)]
    fn provider_tool_names(request: &Value) -> Result<BTreeSet<String>> {
        request
            .get("tools")
            .and_then(Value::as_array)
            .context("real ca provider request omitted tools")?
            .iter()
            .map(|tool| {
                tool.pointer("/function/name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .context("real ca provider request contained an unnamed tool")
            })
            .collect()
    }

    #[cfg(unix)]
    fn assert_real_ca_artifacts(
        paths: &Paths,
        expected_runs: usize,
        expected_successes: usize,
    ) -> Result<()> {
        let mut stdout_artifacts = Vec::new();
        let mut stderr_artifacts = Vec::new();
        for entry in fs::read_dir(&paths.runs)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".ca.stdout.jsonl") {
                stdout_artifacts.push(entry.path());
            } else if name.ends_with(".ca.stderr.log") {
                stderr_artifacts.push(entry.path());
            }
        }
        assert_eq!(stdout_artifacts.len(), expected_runs);
        assert_eq!(stderr_artifacts.len(), expected_runs);
        for path in stderr_artifacts {
            let bytes = fs::read(path)?;
            if !bytes.is_empty() {
                let text = std::str::from_utf8(&bytes)?;
                assert!(text.starts_with("[redacted ca diagnostic:"));
            }
        }
        let mut successes = 0usize;
        for path in stdout_artifacts {
            let records = fs::read_to_string(path)?
                .lines()
                .map(serde_json::from_str::<Value>)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            assert!(!records.is_empty());
            for (sequence, record) in records.iter().enumerate() {
                assert_eq!(record["schema"], "ca.artifact.redacted.v1");
                assert_eq!(record["source_schema_valid"], true);
                assert_eq!(record["sequence"], sequence as u64);
                assert_eq!(record["run_id_present"], true);
                assert_eq!(record["payload_redacted"], true);
            }
            let structured = records
                .iter()
                .position(|record| record["event_type"] == "structured_output");
            let completed = records
                .last()
                .is_some_and(|record| record["event_type"] == "completion");
            if let (Some(structured), true) = (structured, completed) {
                assert!(structured < records.len() - 1);
                successes += 1;
            } else {
                assert!(records.iter().any(|record| {
                    matches!(
                        record["event_type"].as_str(),
                        Some("approval_required" | "failure")
                    )
                }));
            }
        }
        assert_eq!(successes, expected_successes);
        Ok(())
    }

    #[test]
    fn read_only_request_has_only_fixed_tools_and_tool_grants() -> Result<()> {
        let root = temp_root("read-only");
        fs::create_dir_all(root.join(".orchestrator/schemas"))?;
        fs::write(root.join(".orchestrator/schemas/role.json"), "{}")?;
        let paths = Paths::new(&root)?;
        let mut role = role();
        role.sandbox = "danger-full-access".to_string();
        role.approval_policy = "always".to_string();
        role.web_search = "enabled".to_string();
        let request = CaRunRequest {
            role_name: "master",
            role: &role,
            task_id: None,
            prompt: "review",
            schema_path: &root.join(".orchestrator/schemas/role.json"),
            authority: CaRoleAuthority::ReadOnly,
        };
        let machine = build_role_request(
            &paths,
            CaProcessConfig {
                bin: Path::new("/tmp/ca"),
                profile: "codex-team",
                route_provider: "openai",
            },
            &request,
        )?;
        assert_eq!(machine.tools, READ_ONLY_TOOLS);
        assert_eq!(machine.grants.len(), READ_ONLY_TOOLS.len());
        assert!(!machine.tools.iter().any(|tool| matches!(
            tool.as_str(),
            "run_command" | "git" | "delete" | "agents" | "mcp" | "browser" | "web"
        )));
        assert_eq!(
            machine.workspace,
            fs::canonicalize(&root)?.to_str().unwrap()
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn worker_request_rejects_absolute_and_escape_patterns() -> Result<()> {
        for pattern in [
            "/tmp/out",
            "../out",
            "src/../../out",
            r"src\..\..\outside",
            r"C:\outside",
            "C:/outside",
        ] {
            let err = build_authority(
                Path::new("/repo"),
                &CaRoleAuthority::Worker {
                    allowed_files: &[pattern.to_string()],
                },
            )
            .unwrap_err();
            assert!(err.to_string().contains("relative") || err.to_string().contains("escapes"));
        }
        Ok(())
    }

    #[test]
    fn worker_request_rejects_invalid_broad_and_protected_patterns() {
        for pattern in [
            "[invalid",
            "./",
            "**",
            "**/*.rs",
            "*.md",
            "*/safe.txt",
            ".orchestrator/**",
            ".Orchestrator/**",
            ".GIT/config",
            "oracles/**",
            "Oracles/**",
            "ORACLES/nested/*.rs",
            "reference/**",
            "Reference/**",
            "REFERENCE/nested/file.rs",
        ] {
            let error = build_authority(
                Path::new("/repo"),
                &CaRoleAuthority::Worker {
                    allowed_files: &[pattern.to_string()],
                },
            )
            .unwrap_err()
            .to_string();
            assert!(
                error.contains("invalid")
                    || error.contains("protected")
                    || error.contains("literal safe")
                    || error.contains("narrow"),
                "unexpected error for {pattern:?}: {error}"
            );
        }
    }

    #[test]
    fn worker_request_rejects_guard_ignored_roots_case_insensitively() {
        for pattern in [
            "target/generated.rs",
            "Build/output.txt",
            "ANALYSIS/report.md",
            "Benchmarks/source.rs",
        ] {
            let error = build_authority(
                Path::new("/repo"),
                &CaRoleAuthority::Worker {
                    allowed_files: &[pattern.to_string()],
                },
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("guard-ignored"), "{pattern:?}: {error}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn worker_request_rejects_existing_symlink_prefix_and_glob_subtree_escape() -> Result<()> {
        use std::os::unix::fs::symlink;

        let root = temp_root("symlink-scope");
        let outside = temp_root("symlink-scope-outside");
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(&outside)?;
        fs::write(outside.join("escape.rs"), "outside")?;
        symlink(&outside, root.join("src/link"))?;

        for pattern in ["src/link/escape.rs", "src/**/*.rs"] {
            let error = build_authority(
                &root,
                &CaRoleAuthority::Worker {
                    allowed_files: &[pattern.to_string()],
                },
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("symlink"), "{pattern:?}: {error}");
        }

        fs::remove_dir_all(root)?;
        fs::remove_dir_all(outside)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn worker_request_rejects_existing_hardlink_targets_and_glob_subtrees() -> Result<()> {
        let root = temp_root("hardlink-scope");
        let outside = temp_root("hardlink-scope-outside");
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(&outside)?;
        let outside_file = outside.join("sensitive.rs");
        fs::write(&outside_file, "outside")?;
        fs::hard_link(&outside_file, root.join("src/linked.rs"))?;

        for pattern in ["src/linked.rs", "src/**/*.rs"] {
            let error = build_authority(
                &root,
                &CaRoleAuthority::Worker {
                    allowed_files: &[pattern.to_string()],
                },
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("hardlinked"), "{pattern:?}: {error}");
        }
        assert_eq!(fs::read_to_string(&outside_file)?, "outside");
        fs::remove_dir_all(root)?;
        fs::remove_dir_all(outside)?;
        Ok(())
    }

    #[test]
    fn worker_request_uses_expanded_scope_for_both_write_tools() -> Result<()> {
        let allowed = vec!["tracking docs".to_string(), "src/**/*.rs".to_string()];
        let (tools, grants) = build_authority(
            Path::new("/repo"),
            &CaRoleAuthority::Worker {
                allowed_files: &allowed,
            },
        )?;
        assert!(tools.contains(&"apply_patch".to_string()));
        assert!(tools.contains(&"write_file".to_string()));
        assert_eq!(tools.len(), READ_ONLY_TOOLS.len() + 2);
        assert!(grants.iter().any(|grant| matches!(grant, CaGrant::Path { tool, path_pattern } if tool == "apply_patch" && path_pattern == "/repo/NEXT_STEPS.md")));
        assert!(grants.iter().any(|grant| matches!(grant, CaGrant::Path { tool, path_pattern } if tool == "write_file" && path_pattern == "/repo/src/**/*.rs")));
        Ok(())
    }

    #[test]
    fn worker_request_fails_before_launch_when_grants_exceed_contract_limit() {
        let allowed = (0..30)
            .map(|index| format!("src/file-{index}.rs"))
            .collect::<Vec<_>>();
        let error = build_authority(
            Path::new("/repo"),
            &CaRoleAuthority::Worker {
                allowed_files: &allowed,
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("64-grant limit"));
    }

    #[test]
    fn jsonl_requires_one_object_then_later_completion() {
        let valid = br#"{"schema":"ca.headless.v2","sequence":0,"run_id":"r1","event":{"type":"run_start","model":"x"}}
{"schema":"ca.headless.v2","sequence":1,"run_id":"r1","event":{"type":"structured_output","value":{"status":"ok"}}}
{"schema":"ca.headless.v2","sequence":2,"run_id":"r1","event":{"type":"completion","message":"done"}}
"#;
        assert_eq!(parse_ca_jsonl(valid).unwrap()["status"], "ok");

        let duplicate = br#"{"schema":"ca.headless.v2","sequence":0,"run_id":"r1","event":{"type":"structured_output","value":{}}}
{"schema":"ca.headless.v2","sequence":1,"run_id":"r1","event":{"type":"structured_output","value":{}}}
{"schema":"ca.headless.v2","sequence":2,"run_id":"r1","event":{"type":"completion","message":"done"}}
"#;
        assert!(parse_ca_jsonl(duplicate)
            .unwrap_err()
            .to_string()
            .contains("more than once"));
    }

    #[test]
    fn jsonl_rejects_approval_failure_and_sequence_changes() {
        let approval = br#"{"schema":"ca.headless.v2","sequence":0,"run_id":"r1","event":{"type":"approval_required","reason":"TOP_SECRET_CANARY"}}
"#;
        let approval_error = parse_ca_jsonl(approval).unwrap_err().to_string();
        assert!(approval_error.contains("required approval"));
        assert!(!approval_error.contains("TOP_SECRET_CANARY"));
        let sequence = br#"{"schema":"ca.headless.v2","sequence":1,"run_id":"r1","event":{"type":"completion","message":"done"}}
"#;
        assert!(parse_ca_jsonl(sequence)
            .unwrap_err()
            .to_string()
            .contains("monotonic"));
    }

    #[test]
    fn stderr_artifact_redacts_secret_bearing_lines() {
        let sanitized = sanitize_stderr(b"ordinary diagnostic\nopaque-canary-without-marker\n");
        assert!(sanitized.contains("redacted ca diagnostic"));
        assert!(sanitized.contains("2 line(s)"));
        assert!(!sanitized.contains("ordinary diagnostic"));
        assert!(!sanitized.contains("canary"));
    }

    #[test]
    fn stdout_artifact_redacts_model_tool_and_structured_payloads() {
        let canary = "TOP_SECRET_CANARY_917";
        let raw = format!(
            "{{\"schema\":\"ca.headless.v2\",\"sequence\":0,\"run_id\":\"r1\",\"event\":{{\"type\":\"assistant_output\",\"delta\":\"{canary}\"}}}}\n\
             {{\"schema\":\"ca.headless.v2\",\"sequence\":1,\"run_id\":\"r1\",\"event\":{{\"type\":\"tool_start\",\"call_id\":\"c1\",\"name\":\"read_file\",\"arguments\":{{\"path\":\"{canary}\"}}}}}}\n\
             {{\"schema\":\"ca.headless.v2\",\"sequence\":2,\"run_id\":\"r1\",\"event\":{{\"type\":\"tool_finish\",\"call_id\":\"c1\",\"name\":\"read_file\",\"status\":\"success\",\"output\":\"{canary}\"}}}}\n\
             {{\"schema\":\"ca.headless.v2\",\"sequence\":3,\"run_id\":\"r1\",\"event\":{{\"type\":\"structured_output\",\"value\":{{\"answer\":\"{canary}\"}}}}}}\n"
        );
        let sanitized = sanitize_stdout_artifact(raw.as_bytes());
        let text = std::str::from_utf8(&sanitized).unwrap();
        assert!(!text.contains(canary));
        assert!(text.contains("payload_redacted"));
        for line in text.lines() {
            serde_json::from_str::<Value>(line).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn preflight_uses_exact_contract_and_bounded_supervision() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_root("fake-preflight");
        fs::create_dir_all(&root)?;
        let fake = root.join("fake-ca-preflight");
        fs::write(
            &fake,
            "#!/bin/sh\n[ \"$1\" = role-run ] || exit 41\n[ \"$2\" = --profile ] || exit 42\n[ \"$3\" = codex-team ] || exit 43\n[ \"$4\" = --preflight ] || exit 44\nprintf '%s\\n' '{\"schema\":\"ca.role_run_preflight.v1\",\"ready\":true,\"profile\":\"codex-team\",\"sandbox_required\":true,\"routes\":[{\"provider\":\"openai\",\"models\":[\"gpt-test\"]}],\"issues\":[]}'\n",
        )?;
        let mut permissions = fs::metadata(&fake)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake, permissions)?;

        let receipt = run_ca_preflight_with_timeout(
            CaProcessConfig {
                bin: &fake,
                profile: "codex-team",
                route_provider: "openai",
            },
            Duration::from_secs(2),
        )?;
        assert!(receipt.ready);
        assert_eq!(receipt.routes[0].models, ["gpt-test"]);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn preflight_timeout_terminates_the_process_group() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_root("preflight-timeout");
        fs::create_dir_all(&root)?;
        let fake = root.join("fake-ca-preflight-timeout");
        fs::write(&fake, "#!/bin/sh\nwhile :; do sleep 1; done\n")?;
        let mut permissions = fs::metadata(&fake)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake, permissions)?;

        let started = Instant::now();
        let error = run_ca_preflight_with_timeout(
            CaProcessConfig {
                bin: &fake,
                profile: "codex-team",
                route_provider: "openai",
            },
            Duration::from_millis(100),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("preflight timed out"));
        assert!(started.elapsed() < Duration::from_secs(5));
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn role_request_rejects_symlinked_or_outside_output_schema() -> Result<()> {
        let root = temp_root("schema-containment");
        let repo = root.join("repo");
        let outside = root.join("outside.schema.json");
        fs::create_dir_all(repo.join(".orchestrator/schemas"))?;
        fs::write(&outside, "{}")?;
        let link = repo.join(".orchestrator/schemas/link.json");
        std::os::unix::fs::symlink(&outside, &link)?;
        let paths = Paths::new(&repo)?;
        let role = role();

        for schema_path in [&link, &outside] {
            let error = build_role_request(
                &paths,
                CaProcessConfig {
                    bin: Path::new("/tmp/ca"),
                    profile: "codex-team",
                    route_provider: "openai",
                },
                &CaRunRequest {
                    role_name: "master",
                    role: &role,
                    task_id: None,
                    prompt: "review",
                    schema_path,
                    authority: CaRoleAuthority::ReadOnly,
                },
            )
            .unwrap_err()
            .to_string();
            assert!(
                error.contains("non-symlink") || error.contains("contained inside"),
                "unexpected schema error: {error}"
            );
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn real_runner_uses_exact_argv_and_returns_structured_object() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_root("fake-process");
        fs::create_dir_all(root.join(".orchestrator/schemas"))?;
        fs::write(root.join(".orchestrator/schemas/role.json"), "{}")?;
        let fake = root.join("fake-ca");
        fs::write(
            &fake,
            "#!/bin/sh\n[ \"$1\" = role-run ] || exit 41\n[ \"$2\" = --profile ] || exit 42\n[ \"$3\" = codex-team ] || exit 43\nIFS= read -r request\nprintf '%s' \"$request\" | grep '\"schema\":\"ca.role_run.v1\"' >/dev/null || exit 44\nprintf '%s\\n' '{\"schema\":\"ca.headless.v2\",\"sequence\":0,\"run_id\":\"run-1\",\"event\":{\"type\":\"structured_output\",\"value\":{\"answer\":\"yes\"}}}'\nprintf '%s\\n' '{\"schema\":\"ca.headless.v2\",\"sequence\":1,\"run_id\":\"run-1\",\"event\":{\"type\":\"completion\",\"message\":\"done\"}}'\n",
        )?;
        let mut permissions = fs::metadata(&fake)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake, permissions)?;
        let paths = Paths::new(&root)?;
        let role = role();
        let schema = root.join(".orchestrator/schemas/role.json");
        let output = RealCaRunner.run_ca(
            &paths,
            CaProcessConfig {
                bin: &fake,
                profile: "codex-team",
                route_provider: "openai",
            },
            CaRunRequest {
                role_name: "master",
                role: &role,
                task_id: None,
                prompt: "review",
                schema_path: &schema,
                authority: CaRoleAuthority::ReadOnly,
            },
            &CaCancellationToken::default(),
        )?;
        assert_eq!(serde_json::from_str::<Value>(&output)?["answer"], "yes");
        assert_eq!(fs::read_dir(root.join(".orchestrator/runs"))?.count(), 2);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn request_ids_keep_collision_entropy_after_long_labels_are_truncated() {
        let role = "r".repeat(512);
        let task = "t".repeat(512);
        let first = safe_request_id(&role, Some(&task));
        let second = safe_request_id(&role, Some(&task));
        assert!(first.len() <= 128);
        assert!(second.len() <= 128);
        assert_ne!(first, second);
        assert!(is_safe_identifier(&first));
        assert!(is_safe_identifier(&second));
        assert_ne!(
            first.rsplit('-').next(),
            second.rsplit('-').next(),
            "the untruncated sequence suffix must distinguish adjacent runs"
        );
    }

    #[cfg(unix)]
    #[test]
    fn artifact_reservation_rejects_symlinked_runs_directory() -> Result<()> {
        use std::os::unix::fs::symlink;

        let root = temp_root("runs-symlink");
        let outside = temp_root("runs-symlink-outside");
        fs::create_dir_all(root.join(".orchestrator"))?;
        fs::create_dir_all(&outside)?;
        symlink(&outside, root.join(".orchestrator/runs"))?;
        let paths = Paths::new(&root)?;

        let error = ProcessArtifacts::reserve(&paths, "known-run")
            .unwrap_err()
            .to_string();
        assert!(error.contains("without following symlinks"), "{error}");
        assert_eq!(fs::read_dir(&outside)?.count(), 0);

        fs::remove_dir_all(root)?;
        fs::remove_dir_all(outside)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn artifact_reservation_never_follows_or_overwrites_existing_target() -> Result<()> {
        use std::os::unix::fs::symlink;

        let root = temp_root("artifact-symlink");
        let outside = temp_root("artifact-symlink-outside");
        fs::create_dir_all(root.join(".orchestrator/runs"))?;
        fs::create_dir_all(&outside)?;
        let canary = outside.join("canary.txt");
        fs::write(&canary, "outside-canary\n")?;
        symlink(
            &canary,
            root.join(".orchestrator/runs/known-run.ca.stdout.jsonl"),
        )?;
        let paths = Paths::new(&root)?;

        let error = ProcessArtifacts::reserve(&paths, "known-run")
            .unwrap_err()
            .to_string();
        assert!(error.contains("reserve unique ca stdout artifact"));
        assert_eq!(fs::read_to_string(&canary)?, "outside-canary\n");
        assert!(!root
            .join(".orchestrator/runs/known-run.ca.stderr.log")
            .exists());

        fs::remove_dir_all(root)?;
        fs::remove_dir_all(outside)?;
        Ok(())
    }

    #[test]
    fn artifact_collision_fails_without_overwriting_first_run() -> Result<()> {
        let root = temp_root("artifact-collision");
        fs::create_dir_all(root.join(".orchestrator"))?;
        let paths = Paths::new(&root)?;
        let mut first = ProcessArtifacts::reserve(&paths, "known-run")?;
        first.persist(
            &BoundedRead {
                bytes: br#"{"schema":"ca.headless.v2","sequence":0,"run_id":"r","event":{"type":"completion"}}
"#
                .to_vec(),
                overflowed: false,
            },
            &BoundedRead {
                bytes: Vec::new(),
                overflowed: false,
            },
        )?;
        let stdout_path = root.join(".orchestrator/runs/known-run.ca.stdout.jsonl");
        let before = fs::read(&stdout_path)?;

        let error = ProcessArtifacts::reserve(&paths, "known-run")
            .unwrap_err()
            .to_string();
        assert!(error.contains("reserve unique ca stdout artifact"));
        assert_eq!(fs::read(stdout_path)?, before);

        fs::remove_dir_all(root)?;
        Ok(())
    }

    /// Cross-repository compatibility gate for the actual `ca` binary.
    ///
    /// This stays ignored because it requires a separately built, compatible
    /// target checkout. The wrapper gives that binary an empty environment
    /// plus one disposable HOME and a loopback-only credential, so invoking
    /// the test can never consult the user's real profiles or credentials.
    #[cfg(unix)]
    #[test]
    #[ignore = "set CA_BIN to a compatible absolute ca binary to run this hermetic contract test"]
    fn real_ca_binary_preflight_read_only_and_worker_compatibility() -> Result<()> {
        use crate::presentation::render_schema;
        use crate::prompts::{FOREMAN_REVIEW_SCHEMA, MASTER_SCHEMA, PLANNER_SCHEMA, WORKER_SCHEMA};
        use std::os::unix::fs::PermissionsExt;

        let Some(ca_bin) = std::env::var_os("CA_BIN") else {
            eprintln!("skipped real ca compatibility test because CA_BIN is unset");
            return Ok(());
        };
        let ca_bin = fs::canonicalize(&ca_bin).context("canonicalize CA_BIN")?;
        let ca_metadata = fs::metadata(&ca_bin).context("inspect CA_BIN")?;
        if !ca_metadata.is_file() || ca_metadata.permissions().mode() & 0o111 == 0 {
            bail!("CA_BIN must select a regular executable file");
        }

        let root = temp_root("real-ca-compatibility");
        let home = root.join("home");
        let workspace = root.join("workspace");
        fs::create_dir_all(&home)?;
        fs::create_dir_all(&workspace)?;
        let git = Command::new(crate::git_metadata::EXECUTABLE)
            .args(["init", "--quiet"])
            .arg(&workspace)
            .env("HOME", &home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .context("initialize disposable real ca compatibility repository")?;
        if !git.status.success() {
            bail!("failed to initialize disposable real ca compatibility repository");
        }
        let workspace = fs::canonicalize(&workspace)?;
        let schema_dir = workspace.join(".orchestrator/schemas");
        fs::create_dir_all(&schema_dir)?;
        let master_schema = schema_dir.join("master.schema.json");
        let planner_schema = schema_dir.join("planner.schema.json");
        let foreman_schema = schema_dir.join("foreman_review.schema.json");
        let worker_schema = schema_dir.join("worker.schema.json");
        fs::write(&master_schema, render_schema(MASTER_SCHEMA))?;
        fs::write(&planner_schema, render_schema(PLANNER_SCHEMA))?;
        fs::write(&foreman_schema, render_schema(FOREMAN_REVIEW_SCHEMA))?;
        fs::write(&worker_schema, render_schema(WORKER_SCHEMA))?;
        let allowed_path = workspace.join("allowed.txt");
        let outside_scope_path = workspace.join("outside-scope.txt");
        let outside_canary = b"outside-scope-canary\n";
        fs::write(&outside_scope_path, outside_canary)?;

        let master_output = serde_json::json!({
            "project_complete": false,
            "needs_planner": true,
            "roadmap_status": "unchanged",
            "summary": "master-role-ok",
            "warnings": []
        });
        let planner_output = serde_json::json!({
            "project_complete": false,
            "needs_master": false,
            "summary": "planner-role-ok",
            "work_plan": {
                "summary": "bounded plan",
                "goals": [],
                "task_sequence": [],
                "test_strategy": [],
                "pm_return_triggers": []
            },
            "tasks": [],
            "warnings": [],
            "commit_requests": []
        });
        let foreman_output = serde_json::json!({
            "task_id": "INT-148-COMPAT",
            "accepted": true,
            "reason_code": "accepted_clean",
            "needs_master": false,
            "needs_planner": false,
            "project_complete": false,
            "summary": "foreman-role-ok",
            "problems": [],
            "warnings": []
        });
        let worker_output = serde_json::json!({
            "task_id": "INT-148-COMPAT",
            "status": "done",
            "summary": "worker-role-ok",
            "files_changed": ["allowed.txt"],
            "commands_run": [],
            "tests_run": [],
            "handoff_to_rust": [],
            "orchestration_request": null,
            "blocker": "",
            "confidence": "high"
        });

        let responses = vec![
            compatibility_tool_sse(
                "report_structured_output",
                "structured-master",
                serde_json::json!({"value":master_output.clone()}),
            ),
            compatibility_tool_sse(
                "report_structured_output",
                "structured-planner",
                serde_json::json!({"value":planner_output.clone()}),
            ),
            compatibility_tool_sse(
                "report_structured_output",
                "structured-foreman",
                serde_json::json!({"value":foreman_output.clone()}),
            ),
            compatibility_tool_sse(
                "write_file",
                "write-allowed",
                serde_json::json!({
                    "path": path_string(&allowed_path)?,
                    "content": "written through RealCaRunner\n"
                }),
            ),
            compatibility_tool_sse(
                "report_structured_output",
                "structured-worker",
                serde_json::json!({"value":worker_output.clone()}),
            ),
            compatibility_tool_sse(
                "write_file",
                "write-outside-scope",
                serde_json::json!({
                    "path": path_string(&outside_scope_path)?,
                    "content": "forbidden mutation\n"
                }),
            ),
        ];
        let (base_url, provider) = start_real_ca_loopback_provider(responses)?;
        let profile_name = "codex-team-compat";
        let profile = install_real_ca_compat_profile(&home, profile_name, &base_url)?;
        let profile_before = fs::read(&profile)?;
        let ca_wrapper = install_hermetic_ca_wrapper(&root, &home, &ca_bin)?;
        let process_config = || CaProcessConfig {
            bin: ca_wrapper.as_path(),
            profile: profile_name,
            route_provider: "fake",
        };

        let preflight = RealCaRunner.preflight(process_config())?;
        assert!(preflight.ready);
        assert!(preflight.sandbox_required);
        assert_eq!(preflight.profile, profile_name);
        assert!(preflight
            .routes
            .iter()
            .any(|route| { route.provider == "fake" && route.models == ["fake-model"] }));

        let paths = Paths::new(&workspace)?;
        let role = RoleConfig {
            provider: RoleProvider::Ca,
            model: "fake-model".to_string(),
            effort: "high".to_string(),
            sandbox: "read-only".to_string(),
            approval_policy: "never".to_string(),
            web_search: "disabled".to_string(),
            timeout_seconds: 15,
        };
        for (role_name, schema_path, expected) in [
            ("master", master_schema.as_path(), &master_output),
            ("planner", planner_schema.as_path(), &planner_output),
            ("foreman", foreman_schema.as_path(), &foreman_output),
        ] {
            let output = RealCaRunner.run_ca(
                &paths,
                process_config(),
                CaRunRequest {
                    role_name,
                    role: &role,
                    task_id: Some("INT-148-COMPAT"),
                    prompt: "Return the exact structured role compatibility result.",
                    schema_path,
                    authority: CaRoleAuthority::ReadOnly,
                },
                &CaCancellationToken::default(),
            )?;
            assert_eq!(serde_json::from_str::<Value>(&output)?, *expected);
        }

        let allowed_files = vec!["allowed.txt".to_string()];
        let worker_result = RealCaRunner.run_ca(
            &paths,
            process_config(),
            CaRunRequest {
                role_name: "worker",
                role: &role,
                task_id: Some("INT-148-COMPAT"),
                prompt: "Make only the allowed compatibility write and report the result.",
                schema_path: &worker_schema,
                authority: CaRoleAuthority::Worker {
                    allowed_files: &allowed_files,
                },
            },
            &CaCancellationToken::default(),
        )?;
        assert_eq!(
            serde_json::from_str::<Value>(&worker_result)?,
            worker_output
        );
        assert_eq!(
            fs::read_to_string(&allowed_path)?,
            "written through RealCaRunner\n"
        );

        let denied = RealCaRunner.run_ca(
            &paths,
            process_config(),
            CaRunRequest {
                role_name: "worker",
                role: &role,
                task_id: Some("INT-148-DENIED"),
                prompt: "Attempt the supplied write; process-scoped grants must deny it.",
                schema_path: &worker_schema,
                authority: CaRoleAuthority::Worker {
                    allowed_files: &allowed_files,
                },
            },
            &CaCancellationToken::default(),
        );
        assert!(
            denied.is_err(),
            "outside-scope write unexpectedly succeeded"
        );
        assert_eq!(fs::read(&outside_scope_path)?, outside_canary);

        let requests = provider
            .join()
            .map_err(|_| anyhow::anyhow!("real ca loopback provider thread panicked"))??;
        assert_eq!(requests.len(), 6);
        let read_tools = READ_ONLY_TOOLS
            .iter()
            .copied()
            .chain(["report_structured_output"])
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        let worker_tools = READ_ONLY_TOOLS
            .iter()
            .copied()
            .chain(["apply_patch", "write_file", "report_structured_output"])
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        for request in &requests[..3] {
            assert_eq!(provider_tool_names(request)?, read_tools);
        }
        for request in &requests[3..] {
            assert_eq!(provider_tool_names(request)?, worker_tools);
        }
        assert!(requests.iter().all(|request| {
            request.get("model").and_then(Value::as_str) == Some("fake-model")
                && request.get("reasoning_effort").and_then(Value::as_str) == Some("high")
        }));

        assert_real_ca_artifacts(&paths, 5, 4)?;
        assert_eq!(fs::read(&profile)?, profile_before);
        assert!(!home.join(".config/ca/config.toml").exists());
        assert!(!home.join(".local/share/ca").exists());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn real_runner_cleans_up_pipe_holding_descendant_after_successful_child_exit() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_root("successful-child-exit-descendant");
        fs::create_dir_all(root.join(".orchestrator/schemas"))?;
        let schema = root.join(".orchestrator/schemas/role.json");
        fs::write(&schema, "{}")?;
        let descendant_pid_path = root.join("descendant.pid");
        let fake = root.join("fake-ca-success-with-descendant");
        fs::write(
            &fake,
            format!(
                "#!/bin/sh\n\
                 IFS= read -r request\n\
                 sh -c 'trap \"\" TERM HUP; while :; do sleep 1; done' &\n\
                 descendant=$!\n\
                 printf '%s\\n' \"$descendant\" > '{}'\n\
                 printf '%s\\n' '{{\"schema\":\"ca.headless.v2\",\"sequence\":0,\"run_id\":\"run-success\",\"event\":{{\"type\":\"structured_output\",\"value\":{{\"answer\":\"yes\"}}}}}}'\n\
                 printf '%s\\n' '{{\"schema\":\"ca.headless.v2\",\"sequence\":1,\"run_id\":\"run-success\",\"event\":{{\"type\":\"completion\",\"message\":\"done\"}}}}'\n\
                 exit 0\n",
                descendant_pid_path.display(),
            ),
        )?;
        let mut permissions = fs::metadata(&fake)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake, permissions)?;

        let paths = Paths::new(&root)?;
        let role = role();
        let started = Instant::now();
        let output = RealCaRunner.run_ca(
            &paths,
            CaProcessConfig {
                bin: &fake,
                profile: "codex-team",
                route_provider: "openai",
            },
            role_run_request(&role, &schema),
            &CaCancellationToken::default(),
        )?;
        let descendant_pid = wait_for_pid(&descendant_pid_path)?;

        assert!(
            started.elapsed() < Duration::from_secs(8),
            "ca runner did not return within the bounded descendant cleanup window"
        );
        assert_eq!(serde_json::from_str::<Value>(&output)?["answer"], "yes");
        assert_process_exits(descendant_pid);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn real_runner_cancellation_terminates_child_and_grandchild() -> Result<()> {
        let root = temp_root("cancel-process-tree");
        fs::create_dir_all(root.join(".orchestrator/schemas"))?;
        let schema = root.join(".orchestrator/schemas/role.json");
        fs::write(&schema, "{}")?;
        let fake = make_hanging_fake_ca(&root)?;
        let cancellation = CaCancellationToken::default();
        let runner_cancellation = cancellation.clone();
        let runner_root = root.clone();
        let runner_fake = fake.clone();
        let runner_schema = schema.clone();
        let handle = thread::spawn(move || {
            let paths = Paths::new(&runner_root)?;
            let mut role = role();
            role.timeout_seconds = 30;
            RealCaRunner.run_ca(
                &paths,
                CaProcessConfig {
                    bin: &runner_fake,
                    profile: "codex-team",
                    route_provider: "openai",
                },
                role_run_request(&role, &runner_schema),
                &runner_cancellation,
            )
        });

        let child_pid = wait_for_pid(&root.join("child.pid"));
        let grandchild_pid = wait_for_pid(&root.join("grandchild.pid"));
        cancellation.cancel();
        let result = handle
            .join()
            .map_err(|_| anyhow::anyhow!("ca cancellation runner thread panicked"))?;
        let child_pid = child_pid?;
        let grandchild_pid = grandchild_pid?;
        assert!(result.unwrap_err().to_string().contains("was cancelled"));
        assert_process_exits(child_pid);
        assert_process_exits(grandchild_pid);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn real_runner_timeout_terminates_child_and_grandchild() -> Result<()> {
        let root = temp_root("timeout-process-tree");
        fs::create_dir_all(root.join(".orchestrator/schemas"))?;
        let schema = root.join(".orchestrator/schemas/role.json");
        fs::write(&schema, "{}")?;
        let fake = make_hanging_fake_ca(&root)?;
        let paths = Paths::new(&root)?;
        let mut role = role();
        role.timeout_seconds = 1;
        let large_prompt = "x".repeat(128 * 1024);

        let result = RealCaRunner.run_ca(
            &paths,
            CaProcessConfig {
                bin: &fake,
                profile: "codex-team",
                route_provider: "openai",
            },
            CaRunRequest {
                role_name: "worker",
                role: &role,
                task_id: Some("INT-148"),
                prompt: &large_prompt,
                schema_path: &schema,
                authority: CaRoleAuthority::ReadOnly,
            },
            &CaCancellationToken::default(),
        );
        let child_pid = wait_for_pid(&root.join("child.pid"))?;
        let grandchild_pid = wait_for_pid(&root.join("grandchild.pid"))?;
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("timed out after 1 seconds"));
        assert_process_exits(child_pid);
        assert_process_exits(grandchild_pid);
        fs::remove_dir_all(root)?;
        Ok(())
    }
}

fn parse_physical_usage(bytes: &[u8]) -> Result<Option<CaPhysicalUsage>> {
    let mut total = CaPhysicalUsage::default();
    for line in bytes.split(|b| *b == b'\n').filter(|line| !line.is_empty()) {
        let value: Value = serde_json::from_slice(line)?;
        let event = value.get("event").unwrap_or(&value);
        if event.get("type").and_then(Value::as_str) != Some("usage") {
            continue;
        }
        let usage = event.get("usage").context("ca usage missing payload")?;
        let input = usage
            .get("prompt_tokens")
            .and_then(Value::as_u64)
            .context("ca input usage unavailable")?;
        let output = usage
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .context("ca output usage unavailable")?;
        // Learning uses exactly one physical attempt. A provider may publish
        // an interim Usage and repeat the final cumulative snapshot at Finish.
        total.input_tokens = input;
        total.output_tokens = output;
        total.requests = 1;
    }
    Ok((total.requests > 0).then_some(total))
}

#[cfg(test)]
mod learning_usage_tests {
    use super::*;
    #[test]
    fn bounded_single_attempt_usage_reads_v2_and_keeps_missing_unknown() -> Result<()> {
        assert!(parse_physical_usage(b"{\"event\":{\"type\":\"completion\"}}\n")?.is_none());
        let bytes=b"{\"event\":{\"type\":\"usage\",\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":2}}}\n{\"event\":{\"type\":\"usage\",\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":5}}}\n";
        let usage = parse_physical_usage(bytes)?.unwrap();
        assert_eq!(
            (usage.requests, usage.input_tokens, usage.output_tokens),
            (1, 7, 5)
        );
        assert!(parse_physical_usage(
            b"{\"event\":{\"type\":\"usage\",\"usage\":{\"prompt_tokens\":7}}}\n"
        )
        .is_err());
        Ok(())
    }
}
