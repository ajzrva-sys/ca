//! Credentialless real-CLI coverage of cross-provider child dispatch and control.
#![cfg(unix)]

use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const PARENT_MODEL: &str = "deepseek-chat";
// Deliberately configured without pricing: an exact route must not require
// the optional complexity-based router to accept a model.
const CHILD_MODEL: &str = "fixture-child-model";
const CHILD_ROUTE: &str = "route=openai/fixture-child-model";
const HELP_TASK: &str = "CROSS_PROVIDER_HELPER";
const CANCEL_TASK: &str = "CROSS_PROVIDER_CANCEL";
const FOREGROUND_TASK: &str = "CROSS_PROVIDER_FOREGROUND";
const STEERING: &str = "STEERING_FROM_DEEPSEEK_PARENT";
const FILE_TEXT: &str = "pub fn fixture_evidence() -> u32 { 42 }\n";
const CHILD_OUTPUT: &str = "OPENAI_CHILD_READ_AND_STEERING_CONFIRMED";
const FINAL_OUTPUT: &str = "DEEPSEEK_PARENT_CROSS_PROVIDER_CONTROL_CONFIRMED";

#[derive(Clone, Copy, Debug)]
enum Case {
    ControlledChild,
    ForegroundChild,
    InvalidRoutes,
    ConflictingRoute,
}

#[derive(Clone, Debug)]
struct Exchange {
    lane: &'static str,
    path: String,
    body: Value,
}

#[derive(Default)]
struct State {
    exchanges: Vec<Exchange>,
    errors: Vec<String>,
    steering_queued: bool,
    cancellation_observed: bool,
}

struct Provider {
    url: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Provider {
    fn start(lane: &'static str, case: Case, state: Arc<Mutex<State>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                let mut workers = Vec::new();
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let state = Arc::clone(&state);
                            let stop = Arc::clone(&stop);
                            workers.push(thread::spawn(move || {
                                if let Err(error) = serve(stream, lane, case, &state, &stop) {
                                    state.lock().unwrap().errors.push(error);
                                }
                            }));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => {
                            state.lock().unwrap().errors.push(error.to_string());
                            break;
                        }
                    }
                }
                for worker in workers {
                    if worker.join().is_err() {
                        state
                            .lock()
                            .unwrap()
                            .errors
                            .push("provider worker panicked".into());
                    }
                }
            })
        };
        Self {
            url,
            stop,
            thread: Some(thread),
        }
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

fn read_request(stream: &mut TcpStream) -> Result<(String, Value), String> {
    stream.set_nonblocking(false).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if count == 0 || bytes.len() > 64 * 1024 {
            return Err("truncated or oversized HTTP headers".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let path = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or("missing HTTP request path")?
        .to_string();
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
        let count = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("truncated HTTP body".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let body = serde_json::from_slice(&bytes[header_end..header_end + length])
        .map_err(|e| e.to_string())?;
    Ok((path, body))
}

fn content_text(message: &Value) -> String {
    match &message["content"] {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn tool_result(body: &Value, id: &str) -> Option<String> {
    body["messages"].as_array()?.iter().find_map(|message| {
        (message["role"] == "tool" && message["tool_call_id"] == id).then(|| content_text(message))
    })
}

fn agent_id(body: &Value, id: &str) -> Result<String, String> {
    let result = tool_result(body, id).ok_or_else(|| format!("missing spawn result {id}"))?;
    if !result.contains(CHILD_ROUTE) {
        return Err(format!(
            "spawn did not publish actual child route: {result}"
        ));
    }
    result
        .split_whitespace()
        .find(|word| uuid::Uuid::parse_str(word).is_ok())
        .map(str::to_string)
        .ok_or_else(|| format!("spawn omitted child ID: {result}"))
}

fn require_text(result: Option<String>, expected: &[&str]) -> Result<(), String> {
    let result = result.ok_or("expected tool result is absent")?;
    for expected in expected {
        if !result.contains(expected) {
            return Err(format!("tool result lacks {expected:?}: {result}"));
        }
    }
    Ok(())
}

fn wait_for(
    state: &Mutex<State>,
    stop: &AtomicBool,
    predicate: impl Fn(&State) -> bool,
) -> Result<(), String> {
    let started = Instant::now();
    loop {
        if predicate(&state.lock().unwrap()) {
            return Ok(());
        }
        if stop.load(Ordering::SeqCst) || started.elapsed() >= Duration::from_secs(5) {
            return Err("provider fixture synchronization deadline exceeded".into());
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn chat_tool(name: &str, id: &str, arguments: Value) -> String {
    let delta = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}}]},"finish_reason":null}]});
    let finish = json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]});
    format!("data: {delta}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
}

fn chat_text(text: &str) -> String {
    let delta = json!({"choices":[{"delta":{"content":text},"finish_reason":null}]});
    let finish = json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}});
    format!("data: {delta}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
}

fn responses_tool() -> String {
    let arguments = json!({"path":"evidence.rs"}).to_string();
    [
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"child-read","name":"read_file","arguments":""}}),
        json!({"type":"response.function_call_arguments.done","output_index":0,"arguments":arguments}),
        json!({"type":"response.output_item.done","output_index":0,"item":{"id":"fc-child-read","type":"function_call","call_id":"child-read","name":"read_file","arguments":arguments,"status":"completed"}}),
        json!({"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5}}}),
    ].into_iter().map(|event|format!("data: {event}\n\n")).collect()
}

fn responses_text(text: &str) -> String {
    [
        json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":text}),
        json!({"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5}}}),
    ]
    .into_iter()
    .map(|event| format!("data: {event}\n\n"))
    .collect()
}

fn spawn(task: &str, id: &str) -> String {
    chat_tool(
        "spawn_agent",
        id,
        json!({"task":task,"tools":["read_file"],"route":{"provider":"openai","model":CHILD_MODEL}}),
    )
}

fn parent_response(
    body: &Value,
    case: Case,
    state: &Mutex<State>,
    stop: &AtomicBool,
) -> Result<String, String> {
    // The route catalog is read through the same advertised tool the AI uses.
    if tool_result(body, "routes").is_none() {
        let names = body["tools"].as_array().ok_or("missing parent tools")?;
        for name in [
            "list_agent_routes",
            "spawn_agent",
            "await_agent",
            "list_agents",
            "send_agent_message",
            "cancel_agent",
            "task",
        ] {
            if !names.iter().any(|tool| tool["function"]["name"] == name) {
                return Err(format!(
                    "parent cannot discover/control children: missing {name}"
                ));
            }
        }
        return Ok(chat_tool("list_agent_routes", "routes", json!({})));
    }
    let routes: Value = serde_json::from_str(&tool_result(body, "routes").unwrap())
        .map_err(|e| format!("route catalog is not structured JSON: {e}"))?;
    if !routes["routes"].as_array().is_some_and(|routes| {
        routes
            .iter()
            .any(|route| route["provider"] == "openai" && route["model"] == CHILD_MODEL)
    }) {
        return Err(format!(
            "configured cross-provider child absent from catalog: {routes}"
        ));
    }
    if routes.to_string().contains("127.0.0.1") || routes.to_string().contains("api_key") {
        return Err("route discovery leaked endpoint or credential details".into());
    }
    match case {
        Case::ControlledChild => {
            if tool_result(body, "spawn-help").is_none() {
                return Ok(spawn(HELP_TASK, "spawn-help"));
            }
            let helper = agent_id(body, "spawn-help")?;
            if tool_result(body, "steer-help").is_none() {
                wait_for(state, stop, |state| {
                    state.exchanges.iter().any(|entry| {
                        entry.lane == "child" && entry.body.to_string().contains(HELP_TASK)
                    })
                })?;
                return Ok(chat_tool(
                    "send_agent_message",
                    "steer-help",
                    json!({"agent_id":helper,"message":STEERING}),
                ));
            }
            require_text(tool_result(body, "steer-help"), &["Steering", "queued"])?;
            state.lock().unwrap().steering_queued = true;
            if tool_result(body, "await-help").is_none() {
                return Ok(chat_tool(
                    "await_agent",
                    "await-help",
                    json!({"agent_id":helper,"timeout_secs":5}),
                ));
            }
            require_text(
                tool_result(body, "await-help"),
                &["[Completed]", CHILD_OUTPUT, CHILD_ROUTE],
            )?;
            if tool_result(body, "list-help").is_none() {
                return Ok(chat_tool("list_agents", "list-help", json!({})));
            }
            require_text(
                tool_result(body, "list-help"),
                &["[Completed]", CHILD_OUTPUT, CHILD_ROUTE],
            )?;
            if tool_result(body, "spawn-cancel").is_none() {
                return Ok(spawn(CANCEL_TASK, "spawn-cancel"));
            }
            let cancelled = agent_id(body, "spawn-cancel")?;
            if tool_result(body, "cancel-child").is_none() {
                wait_for(state, stop, |state| {
                    state.exchanges.iter().any(|entry| {
                        entry.lane == "child" && entry.body.to_string().contains(CANCEL_TASK)
                    })
                })?;
                return Ok(chat_tool(
                    "cancel_agent",
                    "cancel-child",
                    json!({"agent_id":cancelled}),
                ));
            }
            require_text(
                tool_result(body, "cancel-child"),
                &["Cancellation requested"],
            )?;
            state.lock().unwrap().cancellation_observed = true;
            if tool_result(body, "await-cancel").is_none() {
                return Ok(chat_tool(
                    "await_agent",
                    "await-cancel",
                    json!({"agent_id":cancelled,"timeout_secs":5}),
                ));
            }
            require_text(
                tool_result(body, "await-cancel"),
                &["[Cancelled]", CHILD_ROUTE],
            )?;
            if tool_result(body, "list-final").is_none() {
                return Ok(chat_tool("list_agents", "list-final", json!({})));
            }
            require_text(
                tool_result(body, "list-final"),
                &["[Completed]", "[Cancelled]", CHILD_OUTPUT, CHILD_ROUTE],
            )?;
        }
        Case::ForegroundChild => {
            if tool_result(body, "foreground").is_none() {
                return Ok(chat_tool(
                    "task",
                    "foreground",
                    json!({"subagent_name":"explore","prompt":FOREGROUND_TASK,"route":{"provider":"openai","model":CHILD_MODEL}}),
                ));
            }
            require_text(
                tool_result(body, "foreground"),
                &[CHILD_OUTPUT, CHILD_ROUTE],
            )?;
            if tool_result(body, "list-foreground").is_none() {
                return Ok(chat_tool("list_agents", "list-foreground", json!({})));
            }
            require_text(
                tool_result(body, "list-foreground"),
                &["[Completed]", CHILD_OUTPUT, CHILD_ROUTE],
            )?;
        }
        Case::InvalidRoutes => {
            if tool_result(body, "unknown-provider").is_none() {
                return Ok(chat_tool(
                    "spawn_agent",
                    "unknown-provider",
                    json!({"task":HELP_TASK,"tools":["read_file"],"route":{"provider":"not-configured","model":CHILD_MODEL}}),
                ));
            }
            require_text(
                tool_result(body, "unknown-provider"),
                &[
                    "subagent route is unavailable",
                    "not-configured/fixture-child-model",
                ],
            )?;
            if tool_result(body, "unknown-model").is_none() {
                return Ok(chat_tool(
                    "spawn_agent",
                    "unknown-model",
                    json!({"task":HELP_TASK,"tools":["read_file"],"route":{"provider":"openai","model":"not-configured"}}),
                ));
            }
            require_text(
                tool_result(body, "unknown-model"),
                &["subagent route is unavailable", "openai/not-configured"],
            )?;
            if tool_result(body, "list-rejected").is_none() {
                return Ok(chat_tool("list_agents", "list-rejected", json!({})));
            }
            require_text(tool_result(body, "list-rejected"), &["No child agents"])?;
        }
        Case::ConflictingRoute => {
            return Ok(chat_tool(
                "spawn_agent",
                "route-conflict",
                json!({"task":HELP_TASK,"tools":["read_file"],"route":{"provider":"openai","model":CHILD_MODEL},"complexity":"simple"}),
            ));
        }
    }
    Ok(chat_text(FINAL_OUTPUT))
}

fn child_response(
    body: &Value,
    state: &Mutex<State>,
    stop: &AtomicBool,
) -> Result<Option<String>, String> {
    if body.get("messages").is_some() || body.get("input").is_none() {
        return Err("OpenAI child did not use its Responses protocol".into());
    }
    let text = body.to_string();
    let tools = body["tools"].as_array().ok_or("missing child tools")?;
    let actual: BTreeSet<_> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    let expected = if text.contains(FOREGROUND_TASK) {
        BTreeSet::from([
            "read_file",
            "search_files",
            "glob",
            "list_dir",
            "read_todos",
        ])
    } else {
        BTreeSet::from(["read_file"])
    };
    if actual != expected || actual.len() != tools.len() {
        return Err(format!(
            "child grants differ from its exact assigned set: {actual:?}; expected {expected:?}"
        ));
    }
    if text.contains(CANCEL_TASK) {
        wait_for(state, stop, |state| state.cancellation_observed)?;
        // Cancellation aborts an in-flight real HTTP request. Keeping it open
        // until the parent observes cancellation proves this is active control.
        return Ok(None);
    }
    if text.contains(FOREGROUND_TASK) {
        if !text.contains("function_call_output") {
            return Ok(Some(responses_tool()));
        }
        if !text.contains(FILE_TEXT.trim()) {
            return Err(format!("foreground child lacks real file readback: {body}"));
        }
        return Ok(Some(responses_text(CHILD_OUTPUT)));
    }
    if !text.contains(HELP_TASK) {
        return Err("child lacks the parent's assigned task".into());
    }
    if !text.contains("function_call_output") {
        wait_for(state, stop, |state| state.steering_queued)?;
        return Ok(Some(responses_tool()));
    }
    if !text.contains(STEERING) || !text.contains(FILE_TEXT.trim()) {
        return Err(format!(
            "child failed to receive steering and real file readback: {body}"
        ));
    }
    Ok(Some(responses_text(CHILD_OUTPUT)))
}

fn serve(
    mut stream: TcpStream,
    lane: &'static str,
    case: Case,
    state: &Mutex<State>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let (path, body) = read_request(&mut stream)?;
    state.lock().unwrap().exchanges.push(Exchange {
        lane,
        path: path.clone(),
        body: body.clone(),
    });
    let expected_path = if lane == "parent" {
        "/v1/chat/completions"
    } else {
        "/v1/responses"
    };
    let expected_model = if lane == "parent" {
        PARENT_MODEL
    } else {
        CHILD_MODEL
    };
    if path != expected_path || body["model"] != expected_model {
        return Err(format!(
            "route/protocol drift: {lane} {path} model={}",
            body["model"]
        ));
    }
    let payload = if lane == "parent" {
        Some(parent_response(&body, case, state, stop)?)
    } else {
        child_response(&body, state, stop)?
    };
    if let Some(payload) = payload {
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",payload.len(),payload).map_err(|error|format!("write {lane} response: {error}"))?;
        stream.flush().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn run_bounded(mut command: Command) -> (Output, bool) {
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
        if started.elapsed() >= Duration::from_secs(30) {
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
    (
        Output {
            status,
            stdout,
            stderr,
        },
        timed_out,
    )
}

fn install_config(home: &Path, parent: &str, child: &str) {
    let config_dir = home.join(".config/ca");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        format!(
            r#"
[defaults]
provider="deepseek"
model="{PARENT_MODEL}"
[providers.deepseek]
base_url="{parent}"
api_key=""
protocol="chat_completions"
models=["{PARENT_MODEL}"]
[providers.openai]
base_url="{child}"
api_key=""
protocol="openai_responses"
models=["{CHILD_MODEL}"]
[agent]
max_turns=20
proactive_delegation=false
review_changes=false
keep_awake=false
[harness_improvement]
enabled=false
[learning.dream]
enabled=false
[learning.distill]
enabled=false
"#
        ),
    )
    .unwrap();
}

fn run_case(case: Case) -> Vec<Exchange> {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(workspace.join("evidence.rs"), FILE_TEXT).unwrap();
    let state = Arc::new(Mutex::new(State::default()));
    let mut parent = Provider::start("parent", case, Arc::clone(&state));
    let mut child = Provider::start("child", case, Arc::clone(&state));
    install_config(home.path(), &parent.url, &child.url);
    let executable = std::env::var_os("CA_CROSS_PROVIDER_BINARY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_BIN_EXE_ca")));
    let mut command = Command::new(&executable);
    command.env_clear().env("PATH",std::env::var_os("PATH").unwrap_or_default()).env("HOME",home.path()).env("XDG_CONFIG_HOME",home.path().join(".config")).env("XDG_DATA_HOME",home.path().join(".local/share")).env("XDG_CACHE_HOME",home.path().join(".cache")).env("TERM","dumb").env("LANG","en_US.UTF-8").current_dir(&workspace).args(["--workspace",workspace.to_str().unwrap(),"--trust","--yolo","--exclude-workspace-context","--single-turn","--output-format","jsonl","--prompt","Use the configured provider route catalog and run the explicit cross-provider delegation control fixture."]);
    let (output, timed_out) = run_bounded(command);
    parent.shutdown();
    child.shutdown();
    let state = state.lock().unwrap();
    if let Some(directory) = std::env::var_os("CA_CROSS_PROVIDER_EVIDENCE_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let receipt = json!({
            "case":format!("{case:?}"),
            "executable":executable,
            "credentialless_loopback":true,
            "status":output.status.code(),
            "timed_out":timed_out,
            "stdout":String::from_utf8_lossy(&output.stdout),
            "stderr":String::from_utf8_lossy(&output.stderr),
            "fixture_errors":state.errors,
            "exchanges":state.exchanges.iter().map(|entry|json!({"lane":entry.lane,"path":entry.path,"body":entry.body})).collect::<Vec<_>>(),
        });
        std::fs::write(
            directory.join(format!("{case:?}.json")),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
    }
    assert!(
        !timed_out,
        "CLI timed out: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        state.errors.is_empty(),
        "{case:?}: fixture errors={:?}; stdout={} stderr={}",
        state.errors,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.status.code(),
        Some(if matches!(case, Case::ConflictingRoute) {
            6
        } else {
            0
        }),
        "{case:?}: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "JSONL leaked stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("real CLI emitted non-JSON output"))
        .collect();
    let completions: Vec<_> = events
        .iter()
        .filter(|event| event["event"]["type"] == "completion")
        .collect();
    if matches!(case, Case::ConflictingRoute) {
        assert!(
            completions.is_empty(),
            "invalid dispatch completed: {events:?}"
        );
        let failures: Vec<_> = events
            .iter()
            .filter(|event| event["event"]["type"] == "failure")
            .collect();
        assert_eq!(failures.len(), 1, "{events:?}");
        assert_eq!(failures[0]["event"]["kind"], "provider_protocol");
        assert!(failures[0]["event"]["message"]
            .as_str()
            .unwrap()
            .contains("route and complexity cannot be combined"));
        assert!(
            !events
                .iter()
                .any(|event| event["event"]["type"] == "tool_finish"
                    && event["event"]["call_id"] == "route-conflict"),
            "conflicting dispatch reached tool execution: {events:?}"
        );
    } else {
        assert_eq!(completions.len(), 1, "{events:?}");
        assert_eq!(completions[0]["event"]["message"], FINAL_OUTPUT);
    }
    assert_eq!(
        std::fs::read_to_string(workspace.join("evidence.rs")).unwrap(),
        FILE_TEXT
    );
    assert!(state.exchanges.iter().any(|entry| entry.lane == "parent"));
    eprintln!("{case:?}: {} DeepSeek chat requests, {} OpenAI Responses requests; explicit route/control results validated by the parent fixture",state.exchanges.iter().filter(|entry|entry.lane=="parent").count(),state.exchanges.iter().filter(|entry|entry.lane=="child").count());
    state.exchanges.clone()
}

#[test]
fn deepseek_parent_dispatches_steers_awaits_lists_and_cancels_openai_responses_children() {
    let exchanges = run_case(Case::ControlledChild);
    let child: Vec<_> = exchanges
        .iter()
        .filter(|entry| entry.lane == "child")
        .collect();
    assert_eq!(child.len(), 3, "{child:#?}");
    assert!(child
        .iter()
        .all(|entry| entry.path == "/v1/responses" && entry.body["model"] == CHILD_MODEL));
    assert!(child
        .iter()
        .any(|entry| entry.body.to_string().contains(STEERING)));
    assert!(child
        .iter()
        .any(|entry| entry.body.to_string().contains(CANCEL_TASK)));
}

#[test]
fn deepseek_parent_runs_named_foreground_agent_on_explicit_openai_responses_route() {
    let exchanges = run_case(Case::ForegroundChild);
    assert_eq!(
        exchanges
            .iter()
            .filter(|entry| entry.lane == "child")
            .count(),
        2
    );
    assert!(exchanges
        .iter()
        .filter(|entry| entry.lane == "child")
        .all(|entry| entry.path == "/v1/responses" && entry.body["model"] == CHILD_MODEL));
}

#[test]
fn rejected_explicit_routes_and_complexity_conflict_send_no_child_requests() {
    let exchanges = run_case(Case::InvalidRoutes);
    assert!(
        exchanges.iter().all(|entry| entry.lane == "parent"),
        "invalid route contacted a child or silently fell back: {exchanges:#?}"
    );
    let conflict = run_case(Case::ConflictingRoute);
    assert!(
        conflict.iter().all(|entry| entry.lane == "parent"),
        "conflicting route contacted a child: {conflict:#?}"
    );
    assert_eq!(
        conflict.len(),
        2,
        "conflicting route continued after validation failure: {conflict:#?}"
    );
}
