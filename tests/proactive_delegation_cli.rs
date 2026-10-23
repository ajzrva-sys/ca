//! Real CLI coverage for host admission when the provider never requests a child.
#![cfg(unix)]

use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const PROACTIVE_MARKER: &str = "Host-owned proactive exploration before implementation.";
const HELPER_MARKER: &str = "Host-owned proactive helper result.";
const HELPER_RESULT: &str =
    "EXPLORER_SCOPED_EVIDENCE: checked alpha and beta arithmetic boundaries.";
const DENIED_CANARY: &str = "PERSISTENT_PATH_DENY_CANARY";
const REVIEW_RESULT: &str = "Completed after scoped helper and exact independent review.";
const INITIAL: &str = "pub struct ArithmeticInput {\n    pub left: u32,\n    pub right: u32,\n    pub scale: u32,\n}\n\npub fn combine(input: &ArithmeticInput) -> Option<u32> {\n    input.left.checked_add(input.right)?.checked_mul(input.scale)\n}\n\npub fn answer() -> u32 {\n    let input = ArithmeticInput { left: 20, right: 22, scale: 1 };\n    combine(&input).unwrap_or_default().saturating_sub(42)\n}\n\npub fn boundary_input() -> ArithmeticInput {\n    ArithmeticInput { left: u32::MAX, right: 1, scale: 1 }\n}\n";

#[derive(Clone, Copy, Debug)]
enum Case {
    Enabled,
    DeniedReadPath,
    Disabled,
    DuplicateFile,
    RestrictedCatalog,
    AskSpawn,
    DenySpawn,
    NoApprovalUi,
}

impl Case {
    fn reviews(self) -> bool {
        matches!(
            self,
            Self::Enabled | Self::DeniedReadPath | Self::Disabled | Self::DuplicateFile
        )
    }

    fn explores(self) -> bool {
        matches!(self, Self::Enabled | Self::DeniedReadPath)
    }
}

#[derive(Clone, Debug)]
struct Exchange {
    lane: &'static str,
    body: Value,
    received: Duration,
    responded: Option<Duration>,
    response_call: Option<&'static str>,
    response_text: String,
}

struct Provider {
    url: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    exchanges: Arc<Mutex<Vec<Exchange>>>,
    errors: Arc<Mutex<Vec<String>>>,
}

impl Provider {
    fn start(workspace: PathBuf, case: Case, updated: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let exchanges = Arc::new(Mutex::new(Vec::new()));
        let errors = Arc::new(Mutex::new(Vec::new()));
        let thread = {
            let stop = Arc::clone(&stop);
            let exchanges = Arc::clone(&exchanges);
            let errors = Arc::clone(&errors);
            thread::spawn(move || {
                let started = Instant::now();
                let mut workers = Vec::new();
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let exchanges = Arc::clone(&exchanges);
                            let errors = Arc::clone(&errors);
                            let workspace = workspace.clone();
                            let updated = updated.clone();
                            workers.push(thread::spawn(move || {
                                if let Err(error) =
                                    serve(stream, &workspace, case, &updated, started, &exchanges)
                                {
                                    errors.lock().unwrap().push(error);
                                }
                            }));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => {
                            errors.lock().unwrap().push(error.to_string());
                            break;
                        }
                    }
                }
                for worker in workers {
                    if worker.join().is_err() {
                        errors
                            .lock()
                            .unwrap()
                            .push("provider worker panicked".into());
                    }
                }
            })
        };
        Self {
            url,
            stop,
            thread: Some(thread),
            exchanges,
            errors,
        }
    }

    fn finish(mut self) -> Vec<Exchange> {
        self.shutdown();
        let errors = self.errors.lock().unwrap().clone();
        assert!(errors.is_empty(), "{errors:?}");
        let exchanges = self.exchanges.lock().unwrap().clone();
        assert!(
            !exchanges.is_empty(),
            "real CLI never contacted its isolated provider"
        );
        exchanges
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn request_body(stream: &mut TcpStream) -> Result<Value, String> {
    // The listener is nonblocking only so its shutdown loop remains bounded.
    // Explicitly reset each accepted socket before bounded blocking HTTP I/O.
    stream
        .set_nonblocking(false)
        .map_err(|error| format!("reset accepted socket: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = stream
            .read(&mut chunk)
            .map_err(|error| format!("read HTTP headers after {} bytes: {error}", bytes.len()))?;
        if count == 0 || bytes.len() > 64 * 1024 {
            return Err("truncated or oversized HTTP headers".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .ok_or("missing Content-Length")?;
    if length == 0 || length > 2 * 1024 * 1024 {
        return Err("invalid request body bound".into());
    }
    while bytes.len() < header_end + length {
        let count = stream.read(&mut chunk).map_err(|error| {
            format!(
                "read HTTP body {}/{} bytes: {error}",
                bytes.len() - header_end,
                length
            )
        })?;
        if count == 0 {
            return Err("truncated HTTP body".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    serde_json::from_slice(&bytes[header_end..header_end + length]).map_err(|e| e.to_string())
}

fn message_text(message: &Value) -> String {
    match &message["content"] {
        Value::String(value) => value.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn texts(body: &Value) -> String {
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(message_text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn tool_names(body: &Value) -> BTreeSet<String> {
    body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap().to_owned())
        .collect()
}

fn tool_result<'a>(body: &'a Value, id: &str) -> Option<&'a Value> {
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == id)
}

fn serve(
    mut stream: TcpStream,
    workspace: &Path,
    case: Case,
    updated: &str,
    started: Instant,
    exchanges: &Mutex<Vec<Exchange>>,
) -> Result<(), String> {
    let body = request_body(&mut stream)?;
    if body["model"] != "fixture-model" {
        return Err("route/model inheritance drift".into());
    }
    let names = tool_names(&body);
    let text = texts(&body);
    let lane = if names == BTreeSet::from(["read_file".into()]) && text.contains(PROACTIVE_MARKER) {
        "explorer"
    } else if names.contains("report_structured_output") {
        "reviewer"
    } else {
        "parent"
    };
    let index = {
        let mut records = exchanges.lock().unwrap();
        if records.len() >= 16 {
            return Err("fixture request ceiling exceeded".into());
        }
        let index = records.len();
        records.push(Exchange {
            lane,
            body: body.clone(),
            received: started.elapsed(),
            responded: None,
            response_call: None,
            response_text: String::new(),
        });
        index
    };
    let (call, response_text) = match lane {
        "explorer" => {
            if !case.explores() {
                return Err("Explorer admitted across a disabled/authority boundary".into());
            }
            let alpha = workspace.join("alpha.rs").canonicalize().unwrap();
            let beta = workspace.join("beta.rs").canonicalize().unwrap();
            if !text.contains(alpha.to_str().unwrap()) || !text.contains(beta.to_str().unwrap()) {
                return Err("Explorer task omitted canonical observed source scope".into());
            }
            if matches!(case, Case::DeniedReadPath)
                && tool_result(&body, "explorer-denied").is_none()
            {
                thread::sleep(Duration::from_millis(1200));
                (
                    Some((
                        "read_file",
                        "explorer-denied",
                        json!({"path":workspace.join("gamma.rs").canonicalize().unwrap()}),
                    )),
                    "",
                )
            } else if tool_result(&body, "explorer-read").is_none() {
                if matches!(case, Case::DeniedReadPath) {
                    let denied = message_text(tool_result(&body, "explorer-denied").unwrap());
                    if denied.contains(DENIED_CANARY)
                        || !denied.to_ascii_lowercase().contains("denied")
                    {
                        return Err(format!("Explorer bypassed parent path Deny: {denied}"));
                    }
                } else {
                    thread::sleep(Duration::from_millis(1200));
                }
                (
                    Some(("read_file", "explorer-read", json!({"path": alpha}))),
                    "",
                )
            } else {
                let readback = message_text(tool_result(&body, "explorer-read").unwrap());
                if !readback.contains("ArithmeticInput") || readback.contains("error") {
                    return Err("Explorer did not receive real scoped read_file output".into());
                }
                (None, HELPER_RESULT)
            }
        }
        "reviewer" => {
            let expected = BTreeSet::from(
                [
                    "read_file",
                    "search_files",
                    "glob",
                    "list_dir",
                    "report_structured_output",
                ]
                .map(String::from),
            );
            if names != expected {
                return Err(format!("unsafe reviewer grants: {names:?}"));
            }
            if !text.contains("Independently review the exact turn-owned candidate")
                || !text.contains("alpha.rs")
            {
                return Err("review lacks exact changed-file candidate context".into());
            }
            if tool_result(&body, "review-read").is_none() {
                (
                    Some(("read_file", "review-read", json!({"path":"alpha.rs"}))),
                    "",
                )
            } else {
                if !message_text(tool_result(&body, "review-read").unwrap())
                    .contains(updated.trim())
                {
                    return Err("reviewer readback differs from final candidate".into());
                }
                (
                    Some((
                        "report_structured_output",
                        "review-result",
                        json!({"value":{"approved":true,"findings":[],"summary":"Exact alpha.rs candidate read and reviewed."}}),
                    )),
                    "",
                )
            }
        }
        _ if text.contains("Host-owned independent review for candidate") => (None, REVIEW_RESULT),
        _ if tool_result(&body, "parent-gamma").is_some() => {
            (None, "Parent implementation complete.")
        }
        _ if tool_result(&body, "parent-write").is_some() => (
            Some(("read_file", "parent-gamma", json!({"path":"gamma.rs"}))),
            "",
        ),
        _ if tool_result(&body, "parent-beta").is_some() => {
            // Establish provider-level overlap without relying on Tokio task scheduling order.
            // The child's independent response stays delayed while the parent writes and reads.
            if case.explores() {
                let waiting = Instant::now();
                while !exchanges
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|entry| entry.lane == "explorer")
                {
                    if waiting.elapsed() >= Duration::from_secs(2) {
                        return Err("host did not start Explorer after qualifying reads".into());
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            }
            (
                Some((
                    "write_file",
                    "parent-write",
                    json!({"path":"alpha.rs","content":updated}),
                )),
                "",
            )
        }
        _ if tool_result(&body, "parent-alpha").is_some() => (
            Some((
                "read_file",
                "parent-beta",
                json!({"path": if matches!(case, Case::DuplicateFile) { "./alpha.rs" } else { "./beta.rs" }}),
            )),
            "",
        ),
        _ => (
            Some(("read_file", "parent-alpha", json!({"path":"alpha.rs"}))),
            "",
        ),
    };
    let delta = if let Some((name, id, arguments)) = &call {
        json!({"tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}}]})
    } else {
        json!({"content":response_text})
    };
    let first = json!({"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let last = json!({"choices":[{"index":0,"delta":{},"finish_reason":if call.is_some(){"tool_calls"}else{"stop"}}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}});
    let payload = format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n");
    write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",payload.len(),payload).map_err(|error|format!("write {lane} response: {error}"))?;
    stream
        .flush()
        .map_err(|error| format!("flush {lane} response: {error}"))?;
    let mut records = exchanges.lock().unwrap();
    records[index].responded = Some(started.elapsed());
    records[index].response_call = call.map(|(name, _, _)| name);
    records[index].response_text = response_text.into();
    Ok(())
}

fn run_bounded(mut command: Command) -> Output {
    command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(35) {
            timed_out = true;
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(child.id() as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
            break child.wait().unwrap();
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout_reader.join().unwrap();
    let stderr = stderr_reader.join().unwrap();
    assert!(
        !timed_out,
        "CLI exceeded external bounded process-group deadline; stdout={} stderr={}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    Output {
        status,
        stdout,
        stderr,
    }
}

fn run_case(case: Case) -> Vec<Exchange> {
    assert!(
        INITIAL.len() >= 256
            && INITIAL
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count()
                >= 8
    );
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    for file in ["alpha.rs", "beta.rs"] {
        std::fs::write(workspace.join(file), INITIAL).unwrap();
    }
    std::fs::write(
        workspace.join("gamma.rs"),
        format!("pub fn unrelated() -> u32 {{ 7 }} // {DENIED_CANARY}\n"),
    )
    .unwrap();
    let updated = INITIAL.replace(".saturating_sub(42)", "");
    let provider = Provider::start(workspace.clone(), case, updated.clone());
    let config_dir = home.path().join(".config/ca");
    std::fs::create_dir_all(&config_dir).unwrap();
    let rules = match case {
        Case::AskSpawn => "permission_rules = [{ decision = \"ask\", kind = \"tool\", tool = \"spawn_agent\" }]\n".into(),
        Case::DenySpawn => "permission_rules = [{ decision = \"deny\", kind = \"tool\", tool = \"spawn_agent\" }]\n".into(),
        Case::NoApprovalUi => "approval_rules = [{ AllowTool = \"read_file\" }, { AllowTool = \"write_file\" }]\n".into(),
        Case::DeniedReadPath => format!("permission_rules = [{{ decision = \"deny\", kind = \"path\", tool = \"read_file\", path_pattern = {} }}]\n",serde_json::to_string(&workspace.join("gamma.rs").canonicalize().unwrap()).unwrap()),
        _ => String::new(),
    };
    std::fs::write(config_dir.join("config.toml"),format!("{rules}\n[defaults]\nprovider=\"fixture\"\nmodel=\"fixture-model\"\n[providers.fixture]\nbase_url=\"{}\"\napi_key=\"\"\nmodels=[\"fixture-model\"]\n[agent]\nmax_turns=12\nproactive_delegation={}\nreview_changes={}\nkeep_awake=false\n[harness_improvement]\nenabled=false\n[learning.dream]\nenabled=false\n[learning.distill]\nenabled=false\n",provider.url,!matches!(case,Case::Disabled),case.reviews())).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_ca"));
    command.env_clear().env("PATH",std::env::var_os("PATH").unwrap_or_default()).env("HOME",home.path()).env("XDG_CONFIG_HOME",home.path().join(".config")).env("XDG_DATA_HOME",home.path().join(".local/share")).env("XDG_CACHE_HOME",home.path().join(".cache")).env("TERM","dumb").env("LANG","en_US.UTF-8").current_dir(&workspace).args(["--workspace",workspace.to_str().unwrap(),"--trust","--exclude-workspace-context","--single-turn","--output-format","jsonl","--prompt","Repair the calculation in alpha.rs after checking alpha.rs and beta.rs; also check gamma.rs before completing. Keep the change narrow."]);
    if !matches!(case, Case::NoApprovalUi) {
        command.arg("--yolo");
    }
    if matches!(case, Case::RestrictedCatalog) {
        command.args(["--tools", "read_file,write_file"]);
    }
    let output = run_bounded(command);
    let exchanges = provider.finish();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{case:?}: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "JSONL run leaked stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("non-JSON output in real CLI stream"))
        .collect();
    let completions: Vec<_> = events
        .iter()
        .filter(|event| event["event"]["type"] == "completion")
        .collect();
    assert_eq!(completions.len(), 1, "{case:?}: {events:?}");
    assert_eq!(
        completions[0]["event"]["message"],
        if case.reviews() {
            REVIEW_RESULT
        } else {
            "Parent implementation complete."
        }
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("alpha.rs")).unwrap(),
        updated
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("beta.rs")).unwrap(),
        INITIAL
    );
    assert!(exchanges.iter().all(|entry| !matches!(
        entry.response_call,
        Some("spawn_agent" | "spawn_agent_batch" | "task")
    )));
    if case.explores() {
        let explorers: Vec<_> = exchanges
            .iter()
            .filter(|entry| entry.lane == "explorer")
            .collect();
        assert_eq!(
            explorers.len(),
            if matches!(case, Case::DeniedReadPath) {
                3
            } else {
                2
            },
            "host did not run one real read_file Explorer: {exchanges:#?}"
        );
        let completed_explorer = explorers.last().unwrap();
        assert!(tool_result(&completed_explorer.body, "explorer-read").is_some());
        assert_eq!(completed_explorer.response_text, HELPER_RESULT);
        let parent_work = exchanges
            .iter()
            .find(|entry| {
                entry.lane == "parent" && tool_result(&entry.body, "parent-gamma").is_some()
            })
            .expect("no completed parent read while child was active");
        let parent_read = message_text(tool_result(&parent_work.body, "parent-gamma").unwrap());
        assert!(if matches!(case, Case::DeniedReadPath) {
            parent_read.to_ascii_lowercase().contains("denied")
                && !parent_read.contains(DENIED_CANARY)
        } else {
            parent_read.contains(DENIED_CANARY)
        });
        assert!(
            parent_work.received >= explorers[0].received
                && parent_work.responded.unwrap() < explorers[0].responded.unwrap(),
            "parent work did not overlap delayed child"
        );
        let integrated = exchanges
            .iter()
            .find(|entry| entry.lane == "parent" && texts(&entry.body).contains(HELPER_MARKER))
            .expect("real helper result was not published to parent");
        assert!(texts(&integrated.body).contains(HELPER_RESULT));
        let final_parent = exchanges
            .iter()
            .rev()
            .find(|entry| entry.lane == "parent")
            .unwrap();
        assert_eq!(
            final_parent.body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|message| message_text(message).contains(HELPER_MARKER))
                .count(),
            1,
            "helper publication duplicated across continuations"
        );
        let first_review = exchanges
            .iter()
            .find(|entry| entry.lane == "reviewer")
            .expect("missing final exact Reviewer");
        assert!(
            integrated.received < first_review.received,
            "Reviewer ran before parent integrated helper evidence"
        );
    } else {
        assert!(
            !exchanges.iter().any(|entry| entry.lane == "explorer"),
            "{case:?} admitted a host child"
        );
        assert!(
            !exchanges
                .iter()
                .any(|entry| texts(&entry.body).contains(HELPER_MARKER)),
            "{case:?} invented helper evidence"
        );
    }
    assert_eq!(
        exchanges
            .iter()
            .filter(|entry| entry.lane == "reviewer"
                && tool_result(&entry.body, "review-read").is_none())
            .count(),
        usize::from(case.reviews()),
        "unexpected independent Reviewer count"
    );
    exchanges
}

#[test]
fn host_explorer_runs_without_model_spawn_and_integrates_before_final_review() {
    run_case(Case::Enabled);
}

#[test]
fn host_explorer_inherits_persistent_parent_read_path_denial() {
    run_case(Case::DeniedReadPath);
}

#[test]
fn host_explorer_disabled_and_one_canonical_component_do_not_spawn() {
    run_case(Case::Disabled);
    run_case(Case::DuplicateFile);
}

#[test]
fn host_explorer_respects_catalog_and_persistent_spawn_ceilings() {
    run_case(Case::RestrictedCatalog);
    run_case(Case::AskSpawn);
    run_case(Case::DenySpawn);
}

#[test]
fn host_explorer_does_not_convert_missing_approval_ui_into_authority() {
    run_case(Case::NoApprovalUi);
}
