//! One Rust program calls CA tools through a parent socket.
//!
//! The program is std-only. CA compiles it with `rustc`, then runs the binary
//! under the same sandbox, path, and network policy as `run_command`. Each
//! stub call uses the existing tool handler and the existing approval
//! classification. A call that needs a prompt returns an error to the program
//! instead of waiting on the approval UI. The program cannot call `execute_code`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use uuid::Uuid;

use crate::agent::registry::ToolExecutionResult;
use crate::app::ApprovalMode;

const MAX_SCRIPT_BYTES: usize = 256 * 1024;
const MAX_STDOUT_BYTES: usize = 64 * 1024;
const MAX_STUB_CALLS: usize = 64;
const STUB_TIMEOUT: Duration = Duration::from_secs(30);

static APPROVALS: OnceLock<Mutex<HashMap<Uuid, ApprovalMode>>> = OnceLock::new();

pub struct ApprovalGuard {
    run_id: Uuid,
}

impl Drop for ApprovalGuard {
    fn drop(&mut self) {
        approvals()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(&self.run_id);
    }
}

fn approvals() -> &'static Mutex<HashMap<Uuid, ApprovalMode>> {
    APPROVALS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn bind_approval(run_id: Uuid, mode: ApprovalMode) -> ApprovalGuard {
    approvals()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .insert(run_id, mode);
    ApprovalGuard { run_id }
}

pub fn bound_approval(run_id: Uuid) -> ApprovalMode {
    approvals()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(&run_id)
        .cloned()
        .unwrap_or(ApprovalMode::Allowlist)
}

pub async fn run(
    script: &str,
    workspace: &crate::workspace::WorkspaceContext,
    timeout_secs: u64,
    sandbox_mode: crate::sandbox::SandboxMode,
    network_allowed: bool,
    job_owner: crate::jobs::JobOwner,
    skills: &std::sync::Arc<std::sync::Mutex<crate::skills::SkillRegistry>>,
) -> ToolExecutionResult {
    if script.len() > MAX_SCRIPT_BYTES {
        return ToolExecutionResult::Failure(format!(
            "Error: script exceeds {MAX_SCRIPT_BYTES} bytes"
        ));
    }
    if script.trim().is_empty() {
        return ToolExecutionResult::Failure("Error: script is empty".into());
    }
    let approval = bound_approval(job_owner.run_id);
    let token = Uuid::new_v4().simple().to_string();
    let directory = PathBuf::from(format!("/tmp/ca-ec-{}", &token[..8]));
    if let Err(error) = std::fs::create_dir_all(&directory) {
        return ToolExecutionResult::Failure(format!(
            "Error: could not prepare the tool socket: {error}"
        ));
    }
    if !script.contains("fn main") {
        let _ = std::fs::remove_dir_all(&directory);
        return ToolExecutionResult::Failure(
            "Error: the Rust program needs fn main. Call ca_tool(name, args_json), where args_json is one JSON object.".into(),
        );
    }
    let socket_path = directory.join("s");
    let source_path = directory.join("script.rs");
    let source = rust_program(&socket_path, &token, script);
    if let Err(error) = std::fs::write(&source_path, source) {
        let _ = std::fs::remove_dir_all(&directory);
        return ToolExecutionResult::Failure(format!(
            "Error: could not write the program: {error}"
        ));
    }
    let compile_dir = directory.clone();
    let compiled = tokio::time::timeout(
        Duration::from_secs(timeout_secs.clamp(30, 120)),
        tokio::task::spawn_blocking(move || compile_program(&compile_dir)),
    )
    .await;
    let binary = match compiled {
        Ok(Ok(Ok(binary))) => binary,
        Ok(Ok(Err(error))) => {
            let _ = std::fs::remove_dir_all(&directory);
            return ToolExecutionResult::Failure(bound_text(&error));
        }
        Ok(Err(error)) => {
            let _ = std::fs::remove_dir_all(&directory);
            return ToolExecutionResult::Failure(format!("Error: rustc worker failed: {error}"));
        }
        Err(_) => {
            let _ = std::fs::remove_dir_all(&directory);
            return ToolExecutionResult::Failure("Error: rustc timed out".into());
        }
    };
    let listener = match tokio::net::UnixListener::bind(&socket_path) {
        Ok(listener) => listener,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&directory);
            return ToolExecutionResult::Failure(format!(
                "Error: could not bind the tool socket: {error}"
            ));
        }
    };
    let open = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let server = tokio::spawn(serve_tool_socket(
        listener,
        open.clone(),
        token.clone(),
        approval.clone(),
        workspace.clone(),
        timeout_secs,
        job_owner,
        skills.clone(),
    ));
    let quoted = shell_quote(&binary);
    let command = crate::shell_snapshot::prepared_shell_command(&quoted);
    let mut roots = workspace.execution_roots();
    roots.push(directory.clone());
    let manager = crate::jobs::JobManager::process();
    let started = manager
        .start_sandboxed_owned_with_policy_and_origin(
            command,
            workspace.cwd().to_string_lossy().to_string(),
            false,
            Duration::from_secs(timeout_secs.max(1)),
            roots,
            network_allowed,
            sandbox_mode,
            job_owner,
            crate::jobs::JobExecutionPolicy::default()
                .for_authorized_command(&binary.to_string_lossy(), network_allowed),
            crate::jobs::JobOrigin::ModelTool,
        )
        .await;
    let result = match started {
        Ok(id) => match manager.await_foreground_job_owned(id, job_owner).await {
            Ok(crate::jobs::ForegroundJobOutcome::Finished(job)) => {
                let mut combined = job.stdout.clone();
                if !job.stderr.is_empty() {
                    if !combined.is_empty() && !combined.ends_with('\n') {
                        combined.push('\n');
                    }
                    combined.push_str(&job.stderr);
                }
                let text = bound_text(&combined);
                if matches!(job.status, crate::jobs::JobStatus::Completed { exit_code } if exit_code == 0)
                {
                    ToolExecutionResult::Success(text)
                } else {
                    ToolExecutionResult::Failure(if text.is_empty() {
                        format!("Error: script exited ({})", job.status.label())
                    } else {
                        text
                    })
                }
            }
            Ok(crate::jobs::ForegroundJobOutcome::Promoted(job)) => ToolExecutionResult::Failure(
                format!("Error: script was promoted to job {}", job.id),
            ),
            Err(error) => ToolExecutionResult::Failure(error),
        },
        Err(error) => ToolExecutionResult::Failure(error),
    };
    open.store(false, std::sync::atomic::Ordering::Release);
    server.abort();
    let _ = std::fs::remove_dir_all(&directory);
    result
}

fn shell_quote(path: &std::path::Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

fn bound_text(text: &str) -> String {
    if text.len() <= MAX_STDOUT_BYTES {
        return text.to_string();
    }
    let mut end = MAX_STDOUT_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[stdout truncated to {MAX_STDOUT_BYTES} bytes]",
        &text[..end]
    )
}

fn rust_program(socket: &std::path::Path, token: &str, script: &str) -> String {
    format!(
        r#"mod ca_host {{
    use std::io::{{BufRead, BufReader, Write}};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    const SOCKET: &str = {socket:?};
    const TOKEN: &str = {token:?};

    pub fn ca_tool(name: &str, args_json: &str) -> Result<String, String> {{
        let mut stream = UnixStream::connect(SOCKET).map_err(|error| error.to_string())?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
        let mut payload = String::from("{{\"token\":");
        payload.push_str(&json_string(TOKEN));
        payload.push_str(",\"tool\":");
        payload.push_str(&json_string(name));
        payload.push_str(",\"args\":");
        payload.push_str(args_json);
        payload.push('}}');
        payload.push('\n');
        stream
            .write_all(payload.as_bytes())
            .map_err(|error| error.to_string())?;
        stream.flush().map_err(|error| error.to_string())?;
        let mut line = String::new();
        BufReader::new(stream)
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        decode_response(line.trim())
    }}

    fn json_string(value: &str) -> String {{
        let mut out = String::from("\"");
        for ch in value.chars() {{
            match ch {{
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                other if other.is_control() => out.push_str(&format!("\\u{{:04x}}", other as u32)),
                other => out.push(other),
            }}
        }}
        out.push('"');
        out
    }}

    fn decode_response(body: &str) -> Result<String, String> {{
        let ok = body.contains("\"ok\":true");
        let key = if ok {{ "\"output\":" }} else {{ "\"error\":" }};
        let Some(start) = body.find(key) else {{
            return Err(if body.is_empty() {{
                "empty tool response".into()
            }} else {{
                body.to_string()
            }});
        }};
        let rest = &body[start + key.len()..];
        let value = if let Some(text) = rest.strip_prefix('"') {{
            unescape_json_string(text)?
        }} else if rest.starts_with("null") {{
            String::new()
        }} else {{
            return Err(body.to_string());
        }};
        if ok {{ Ok(value) }} else {{ Err(if value.is_empty() {{ "tool failed".into() }} else {{ value }}) }}
    }}

    fn unescape_json_string(text: &str) -> Result<String, String> {{
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(ch) = chars.next() {{
            if ch == '"' {{
                return Ok(out);
            }}
            if ch != '\\' {{
                out.push(ch);
                continue;
            }}
            match chars.next() {{
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('u') => {{
                    let hex: String = chars.by_ref().take(4).collect();
                    let code = u32::from_str_radix(&hex, 16).map_err(|_| "bad unicode escape".to_string())?;
                    out.push(char::from_u32(code).unwrap_or('\u{{FFFD}}'));
                }}
                _ => return Err("bad string escape".into()),
            }}
        }}
        Err("unterminated tool response".into())
    }}
}}
use ca_host::ca_tool;

{script}
"#
    )
}

fn compile_program(directory: &std::path::Path) -> Result<PathBuf, String> {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let source = directory.join("script.rs");
    let output = std::process::Command::new(rustc)
        .arg("--edition=2021")
        .arg("-C")
        .arg("opt-level=0")
        .arg("-C")
        .arg("debuginfo=0")
        .arg("-C")
        .arg("incremental=false")
        .arg("--out-dir")
        .arg(directory)
        .arg(&source)
        .output()
        .map_err(|error| format!("Error: rustc is unavailable: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!("Error: rustc failed\n{stderr}{stdout}"));
    }
    let binary = directory.join("script");
    if !binary.is_file() {
        return Err("Error: rustc produced no binary".into());
    }
    Ok(binary)
}

#[allow(clippy::too_many_arguments)]
async fn serve_tool_socket(
    listener: tokio::net::UnixListener,
    open: std::sync::Arc<std::sync::atomic::AtomicBool>,
    token: String,
    approval: ApprovalMode,
    workspace: crate::workspace::WorkspaceContext,
    timeout_secs: u64,
    job_owner: crate::jobs::JobOwner,
    skills: std::sync::Arc<std::sync::Mutex<crate::skills::SkillRegistry>>,
) {
    let calls = std::sync::atomic::AtomicUsize::new(0);
    loop {
        if !open.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        let accepted = tokio::time::timeout(Duration::from_millis(200), listener.accept()).await;
        let Ok(Ok((stream, _))) = accepted else {
            if !open.load(std::sync::atomic::Ordering::Acquire) {
                break;
            }
            continue;
        };
        if calls.fetch_add(1, std::sync::atomic::Ordering::AcqRel) >= MAX_STUB_CALLS {
            let (_read, mut write) = stream.into_split();
            let _ = write_json(
                &mut write,
                &json!({"ok": false, "error": "execute_code stub call limit reached"}),
            )
            .await;
            continue;
        }
        handle_stub(
            stream,
            &token,
            &approval,
            &workspace,
            timeout_secs,
            job_owner,
            &skills,
        )
        .await;
    }
}

async fn handle_stub(
    stream: tokio::net::UnixStream,
    token: &str,
    approval: &ApprovalMode,
    workspace: &crate::workspace::WorkspaceContext,
    timeout_secs: u64,
    job_owner: crate::jobs::JobOwner,
    skills: &std::sync::Arc<std::sync::Mutex<crate::skills::SkillRegistry>>,
) {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read);
    let mut line = String::new();
    let read = tokio::time::timeout(Duration::from_secs(5), lines.read_line(&mut line)).await;
    let request = match read {
        Ok(Ok(_)) => serde_json::from_str::<Value>(line.trim()).unwrap_or(Value::Null),
        _ => {
            let _ = write_json(
                &mut write,
                &json!({"ok": false, "error": "malformed tool request"}),
            )
            .await;
            return;
        }
    };
    if request.get("token").and_then(Value::as_str) != Some(token) {
        let _ = write_json(
            &mut write,
            &json!({"ok": false, "error": "tool socket token rejected"}),
        )
        .await;
        return;
    }
    let name = request.get("tool").and_then(Value::as_str).unwrap_or("");
    let args = request.get("args").cloned().unwrap_or_else(|| json!({}));
    let response = match stub_result(
        name,
        &args,
        approval,
        workspace,
        timeout_secs,
        job_owner,
        skills,
    )
    .await
    {
        Ok(output) => json!({"ok": true, "output": output}),
        Err(error) => json!({"ok": false, "error": error}),
    };
    let _ = write_json(&mut write, &response).await;
}

async fn write_json(
    write: &mut tokio::net::unix::OwnedWriteHalf,
    value: &Value,
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut bytes = serde_json::to_vec(value).unwrap_or_else(|_| b"{\"ok\":false}".to_vec());
    bytes.push(b'\n');
    write.write_all(&bytes).await
}

async fn stub_result(
    name: &str,
    args: &Value,
    approval: &ApprovalMode,
    workspace: &crate::workspace::WorkspaceContext,
    timeout_secs: u64,
    job_owner: crate::jobs::JobOwner,
    skills: &std::sync::Arc<std::sync::Mutex<crate::skills::SkillRegistry>>,
) -> Result<String, String> {
    if name == "execute_code" {
        return Err("execute_code cannot call execute_code".into());
    }
    let registry = crate::agent::registry::ToolRegistry::builtins();
    if registry.get(name).is_none() {
        return Err(format!("unknown tool: {name}"));
    }
    if let Err(error) = registry.validate(name, args) {
        return Err(error.to_string());
    }
    if crate::tool_needs_approval(name, args, approval) {
        return Err("approval required; the script cannot raise an approval prompt".into());
    }
    let execution = tokio::time::timeout(
        STUB_TIMEOUT.min(Duration::from_secs(timeout_secs.max(1))),
        crate::agent::tools::execute_from_script(
            name,
            args,
            workspace,
            timeout_secs,
            crate::sandbox::SandboxMode::Off,
            false,
            job_owner,
            skills,
        ),
    )
    .await
    .map_err(|_| "tool call timed out".to_string())?;
    if execution.is_success() {
        Ok(execution.model_text().to_string())
    } else {
        Err(execution.model_text().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_script_reads_an_allowed_file_and_a_rejected_call_returns() {
        let root = std::env::temp_dir().join(format!("ca-exec-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("note.txt"), "ALLOWED_NOTE\n").unwrap();
        let workspace =
            crate::workspace::WorkspaceContext::from_primary(root.clone(), root.clone());
        let owner = crate::jobs::JobOwner::new(None);
        let _guard = bind_approval(owner.run_id, ApprovalMode::AutoReview);
        let skills =
            std::sync::Arc::new(std::sync::Mutex::new(crate::skills::SkillRegistry::empty()));
        let script = r#"
fn main() {
    match ca_tool("read_file", "{\"path\":\"note.txt\"}") {
        Ok(text) => println!("{text}"),
        Err(error) => println!("read failed {error}"),
    }
    match ca_tool("web_search", "{\"query\":\"example\"}") {
        Ok(text) => println!("{text}"),
        Err(error) => println!("REJECTED {error}"),
    }
    match ca_tool("execute_code", "{\"script\":\"fn main() {}\"}") {
        Ok(text) => println!("{text}"),
        Err(error) => println!("RECURSIVE {error}"),
    }
}
"#;
        let result = run(
            script,
            &workspace,
            90,
            crate::sandbox::SandboxMode::Off,
            false,
            owner,
            &skills,
        )
        .await;
        let text = result.model_text().to_string();
        assert!(text.contains("ALLOWED_NOTE"), "{text}");
        assert!(text.contains("REJECTED"), "{text}");
        assert!(text.contains("approval required"), "{text}");
        assert!(text.contains("RECURSIVE"), "{text}");
        assert!(text.contains("cannot call execute_code"), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
