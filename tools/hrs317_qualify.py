#!/usr/bin/env python3
"""Paired HRS-317 normal-entry qualification; never reads provider credentials.

All live calls require --candidate and --output. --prepare-only and --self-test
do not contact a provider. Receipts distinguish missing evidence from a pass.
"""
from __future__ import annotations

import argparse
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BASELINE = ROOT / "scratch/HRS-317/baseline-qualified-ca"
PUBLIC_PYTHON = "hrs317-python"
RUNTIME_WRAPPER = ".hrs317-runtime"
CITATION_FILES = {"HRS317_E1": "target.py", "HRS317_E2": "beta.txt",
                  "HRS317_E3": "gamma.txt", "HRS317_E4": "build-diagnostics.log"}
SCHEMA = {
    "type": "object", "additionalProperties": False,
    "required": ["status", "answer", "evidence"],
    "properties": {
        "status": {"type": "string", "enum": ["complete", "incomplete"]},
        "answer": {"type": "string", "maxLength": 4096},
        "evidence": {"type": "array", "maxItems": 8, "items": {
            "type": "object", "additionalProperties": False,
            "required": ["path", "start_line", "end_line", "quote"],
            "properties": {"path": {"type": "string", "enum": list(CITATION_FILES)},
                           "start_line": {"type": "integer", "minimum": 1},
                           "end_line": {"type": "integer", "minimum": 1},
                           "quote": {"type": "string", "minLength": 1, "maxLength": 4096}}}}}}


def dump(path: Path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")


def sha(path: Path):
    with path.open("rb") as stream:
        digest = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
        return digest.hexdigest()


def plan():
    cases = [(feature, case, case == 2) for feature in ("narrow", "context", "logs")
             for case in range(3)] + [("combined", 2, True)]
    runs = []
    for feature, case, held_out in cases:
        for repetition in range(3):
            # Counterbalance order without selecting favorable outcomes.
            lanes = ("baseline", "candidate") if repetition % 2 == 0 else ("candidate", "baseline")
            for lane in lanes:
                runs.append({"feature": feature, "case": case, "held_out": held_out,
                             "repetition": repetition, "lane": lane,
                             "id": f"{feature}-{case}-r{repetition}-{lane}"})
    assert len(runs) == 60
    return runs


def config(workspace: Path):
    return f'''trusted_workspaces={json.dumps([str(workspace)])}
[defaults]
provider="openai-codex"
model="gpt-6.1-sol"
[providers.openai-codex]
base_url="https://chatgpt.com/backend-api/codex"
api_key=""
protocol="openai_responses"
models=["gpt-6.1-sol"]
[providers.openai-codex.timeout]
connect_timeout_secs=30
max_attempts=1
headers_timeout_secs=75
first_event_timeout_secs=60
no_progress_timeout_secs=75
[model_profiles."openai-codex/gpt-6.1-sol"]
supports_tools=true
reasoning_efforts=["low","medium","high"]
default_reasoning_effort="low"
[provider_filter]
enabled=true
structural=true
pages="latin"
[provider_filter.hide]
filenames=true
[agent]
review_changes=false
proactive_delegation=false
max_turns=24
subagent_max_turns=12
subagent_children_per_turn=2
subagent_concurrency=1
subagent_max_depth=1
subagent_runtime_secs=150
command_timeout_secs=30
keep_awake=false
[harness_improvement]
enabled=false
[learning.dream]
enabled=false
[learning.distill]
enabled=false
'''


def runtime_wrapper(workspace: Path):
    path = workspace.resolve(strict=True) / RUNTIME_WRAPPER
    contents = ('#!/bin/sh\nexec ' + shlex.quote(str(Path(sys.executable).resolve(strict=True))) + ' "$@"\n').encode()
    if path.is_symlink():
        raise ValueError("Fixture runtime wrapper must not be a symlink")
    if path.exists():
        if path.read_bytes() != contents or path.stat().st_mode & 0o777 != 0o755:
            raise ValueError("Protected fixture runtime wrapper changed")
    else:
        path.write_bytes(contents)
        path.chmod(0o755)
    return path


def environment(home: Path, temporary: Path):
    env = {key: os.environ[key] for key in ("PATH", "LANG", "LC_ALL", "TERM") if key in os.environ}
    public_tools = home / "public-tools"
    public_tools.mkdir(mode=0o700, exist_ok=True)
    launcher = public_tools / PUBLIC_PYTHON
    wrapper = home.parent / "workspace" / RUNTIME_WRAPPER
    if not wrapper.is_file() or wrapper.is_symlink():
        raise ValueError("Protected workspace runtime wrapper must be prepared before environment setup")
    wrapper = wrapper.resolve(strict=True)
    if launcher.is_symlink():
        if launcher.resolve(strict=True) != wrapper:
            raise ValueError("Disposable public Python launcher changed its interpreter")
    elif launcher.exists():
        raise ValueError("Disposable public Python launcher must be the host-pinned symlink")
    else:
        launcher.symlink_to(wrapper)
    env["PATH"] = str(public_tools) + os.pathsep + env.get("PATH", "/usr/bin:/bin")
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"),
               XDG_DATA_HOME=str(home / ".local/share"), XDG_CACHE_HOME=str(home / ".cache"),
               TMPDIR=str(temporary), CODEX_HOME=os.environ.get("CODEX_HOME", "/Users/aji/.codex"),
               RUSTUP_HOME=os.environ.get("RUSTUP_HOME", "/Users/aji/.rustup"),
               CARGO_HOME=str(home / ".cargo"), TERM="xterm-256color", LANG="en_US.UTF-8",
               GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
               GIT_TERMINAL_PROMPT="0", PYTHONDONTWRITEBYTECODE="1", CARGO_NET_OFFLINE="true")
    return env


def python_preflight(env):
    interpreter = Path(sys.executable).resolve(strict=True)
    expected_sha256 = sha(interpreter)
    launcher = Path(shutil.which(PUBLIC_PYTHON, path=env["PATH"]))
    wrapper = launcher.resolve(strict=True)
    expected_wrapper = ('#!/bin/sh\nexec ' + shlex.quote(str(interpreter)) + ' "$@"\n').encode()
    if (wrapper.name != RUNTIME_WRAPPER or wrapper.is_symlink() or not wrapper.is_file()
            or wrapper.read_bytes() != expected_wrapper or wrapper.stat().st_mode & 0o777 != 0o755):
        raise ValueError("Public Python launcher is not the exact protected executable wrapper")
    probe = subprocess.run([PUBLIC_PYTHON, "-I", "-B", "-c",
                            "import json,os,sys; print(json.dumps({'runtime':os.path.realpath(sys.executable),'version':sys.version}))"],
                           env=env, capture_output=True, text=True, timeout=10, check=True)
    observed = json.loads(probe.stdout)
    if (Path(observed["runtime"]).resolve(strict=True) != interpreter
            or observed["version"] != sys.version or sha(Path(observed["runtime"])) != expected_sha256):
        raise ValueError("Public Python transport does not resolve to the exact pinned host interpreter")
    return {"public_name": PUBLIC_PYTHON, "resolved_runtime": str(interpreter),
            "runtime_sha256": expected_sha256, "version": observed["version"],
            "public_launcher": shutil.which(PUBLIC_PYTHON, path=env["PATH"]),
            "wrapper_path": str(wrapper), "wrapper_sha256": sha(wrapper), "wrapper_mode": "0755",
            "no_workspace_checker_changes": True}


def narrow_fixture(workspace: Path, case: int, lane: str):
    wrapper = runtime_wrapper(workspace)
    variants = [
        ("value, floor", "    return min(value, floor)\n", "return a value at least floor",
         "assert transform(2, 5) == 5\nassert transform(9, 5) == 9\nassert transform(-3, -1) == -1\n"),
        ("text, start, end", "    return text[start:end + 1]\n", "use an exclusive end offset",
         "assert transform('abcdef', 1, 3) == 'bc'\nassert transform('abc', 1, 1) == ''\nassert transform('αβγ', 0, 2) == 'αβ'\n"),
        ("values", "    return sorted(set(values))\n", "remove duplicates while preserving first appearance order",
         "assert transform([3, 1, 3, 2, 1]) == [3, 1, 2]\nassert transform(['b', 'a', 'b']) == ['b', 'a']\nassert transform([]) == []\n")]
    signature, bad, task, assertions = variants[case]
    original = f"def transform({signature}):\n{bad}\nPROTECTED = 'exterior317-{case}'\n"
    (workspace / "target.py").write_text(original)
    checker = "from target import transform\n" + assertions + "print('CHECK317_OK')\n"
    (workspace / "check_target.py").write_text(checker)
    public_command = shlex.join([PUBLIC_PYTHON, "check_target.py"])
    typed_command = shlex.join([str(wrapper), "check_target.py"])
    command = typed_command if lane == "candidate" else public_command
    contract = {"path": "target.py", "source_sha256": hashlib.sha256(original.encode()).hexdigest(),
                "start_line": 2, "end_line": 2, "context": task,
                "checks": [{"argv": [str(wrapper), "check_target.py"], "cwd": ".", "timeout_secs": 10}],
                "max_attempts": 3}
    treatment = (f"Supply repair_contract exactly {json.dumps(contract)}; omit output_schema. "
                 if lane == "candidate" else
                 "Use an ordinary child without repair_contract. The child edits the known target and runs the pinned check. ")
    prompt = (f"Repair this known single-line Python defect: {task}. The exact current source is:\n{original}\n"
              f"Delegate exactly once using spawn_agent, tools=[read_file,edit_file,run_command], commands=[{json.dumps(command)}], "
              f"and the current route openai-codex/gpt-6.1-sol. Use shared isolation, omit assigned_files, grant no network or nested children. {treatment}"
              f"Only line 2 of target.py may change. Keep all other bytes and check_target.py unchanged. "
              f"The fixed acceptance command is {command}. Await the child and report its verified result; do not independently edit. "
              "Return complete only after that command passes, and cite the repaired target line in the requested final schema.")
    return prompt, {"kind": "narrow", "original": original, "expected_answer": [],
                    "check_transport": {"public_command": public_command, "typed_command": typed_command,
                                        "selected_grant_command": command,
                                        "same_runtime_sha256": sha(Path(sys.executable).resolve(strict=True)),
                                        "reason": "Ordinary shell requires a public executable token; typed narrow checks retain their required absolute program"},
                    "allowed_changes": ["target.py"], "check_command": [str(wrapper), "check_target.py"],
                    "runtime_wrapper": RUNTIME_WRAPPER, "runtime_wrapper_mode": 0o755}


def context_fixture(workspace: Path, case: int):
    facts = {"alpha.txt": "ALPHA317=amber", "beta.txt": "BETA317=violet", "gamma.txt": "GAMMA317=teal"}
    for path, fact in facts.items():
        (workspace / path).write_text(fact + "\n" + "context317 padding is ordinary evidence. " * 512 + "\n")
    repeated = ("First read alpha.txt, beta.txt and gamma.txt in three separate tool turns. Then read alpha.txt "
                "four more times in four separate tool turns. Do not combine reads, delegate, or replace the mandated read sequence with a shell summary. ")
    commands = []
    if case == 0:
        action = "Report the exact current BETA317 and GAMMA317 assignments, citing each source file's first line."
        wanted = ["BETA317=violet", "GAMMA317=teal"]
    elif case == 1:
        (workspace / "check_state.py").write_text("from pathlib import Path\nassert 'GAMMA317='+__import__('sys').argv[1] in Path('gamma.txt').read_text()\nprint('CURRENT317_OK')\n")
        old = shlex.join([PUBLIC_PYTHON, "check_state.py", "teal"])
        new = shlex.join([PUBLIC_PYTHON, "check_state.py", "cobalt"])
        commands = [old, new]
        action = (f"Run {old}; its success describes the old state. Next use edit_file to replace only GAMMA317=teal "
                  f"with GAMMA317=cobalt in gamma.txt. Read gamma.txt again and run {new}. Report BETA317=violet and "
                  "the current gamma assignment. Cite current first lines; an old passing check cannot verify an edited revision.")
        wanted = ["BETA317=violet", "GAMMA317=cobalt"]
    else:
        (workspace / "external_change.py").write_text("from pathlib import Path\np=Path('gamma.txt')\np.write_text(p.read_text().replace('GAMMA317=teal','GAMMA317=indigo'))\n")
        change = shlex.join([PUBLIC_PYTHON, "external_change.py"])
        commands = [change]
        action = (f"Run {change} exactly once. Its effect requires inspection. Read gamma.txt again, then report "
                  "the current beta and gamma assignments with current first-line citations.")
        # Saved filename privacy executes shell commands in an independent copy.
        # Inspection must establish that the original source is still unchanged.
        wanted = ["BETA317=violet", "GAMMA317=teal"]
    return repeated + action + " Preserve every other byte and file. Use the final schema.", {
        "kind": "context", "expected_answer": wanted, "allowed_changes": [] if case == 0 else ["gamma.txt"],
        "required_commands": commands, "initial_gamma": (workspace / "gamma.txt").read_text()}


def log_fixture(workspace: Path, case: int, lane: str):
    symbol = f"missing_symbol317_{case}"
    size = (1, 4, 15)[case] * 1024 * 1024
    (workspace / "Cargo.toml").write_text('[package]\nname="hrs317_log_fixture"\nversion="0.1.0"\nedition="2021"\nbuild="build.rs"\n[lib]\npath="lib.rs"\n[workspace]\n')
    (workspace / "lib.rs").write_text("pub fn fixture() {}\n")
    (workspace / "broken.rs").write_text(f"pub fn broken() -> i32 {{\n    {symbol}()\n}}\n")
    # This is an actual failed Cargo build. Explicit synthetic build-script
    # padding surrounds genuine rustc diagnostics; it is not a fake compiler log.
    build = r'''use std::{env, fs::File, io::Write, process::{Command, exit}};
fn emit(log: &mut File, text: &str) { log.write_all(text.as_bytes()).unwrap(); eprint!("{}", text); }
fn main() {
 let root=env::var("CARGO_MANIFEST_DIR").unwrap();
 let mut log=File::create(format!("{}/build-diagnostics.log",root)).unwrap();
 let pad="HRS317 synthetic build progress padding; no diagnostic or instruction; completed background compilation phase. ----------------------------------------------------------\n";
 for _ in 0..PADDING/pad.len() { emit(&mut log,pad); }
 let output=Command::new(env::var("RUSTC").unwrap_or("rustc".into())).current_dir(&root).args(["--edition=2021","--crate-type=lib","--emit=metadata","--out-dir",&env::var("OUT_DIR").unwrap(),"broken.rs"]).output().unwrap();
 emit(&mut log,&String::from_utf8_lossy(&output.stderr));
 for _ in 0..PADDING/pad.len() { emit(&mut log,pad); }
 assert!(!output.status.success()); exit(1);
}
'''.replace("PADDING", str(size // 2))
    (workspace / "build.rs").write_text(build)
    # Stored-data fixture: an actual compiler produces the immutable seed. The
    # one normal Cargo invocation must reproduce it in its private command view.
    prepared = workspace.parent / "compiler-fixture"
    prepared.mkdir()
    command = [shutil.which("rustc") or "rustc", "--edition=2021", "--crate-type=lib", "--emit=metadata",
               "--out-dir", str(prepared), "broken.rs"]
    started = time.monotonic()
    diagnostic = subprocess.run(command, cwd=workspace, capture_output=True, timeout=15,
                                env={"PATH": os.environ.get("PATH", "/usr/bin:/bin"),
                                     "HOME": str(workspace.parent / "runtime"), "LANG": "en_US.UTF-8"})
    if diagnostic.returncode == 0 or b"error[E0425]" not in diagnostic.stderr or symbol.encode() not in diagnostic.stderr:
        raise ValueError("Genuine compiler diagnostic fixture did not reproduce E0425")
    padding = b"HRS317 synthetic build progress padding; no diagnostic or instruction; completed background compilation phase. ----------------------------------------------------------\n"
    half = padding * ((size // 2) // len(padding))
    (workspace / "build-diagnostics.log").write_bytes(half + diagnostic.stderr + half)
    dump(prepared / "receipt.json", {"argv": command, "exit_code": diagnostic.returncode,
                                    "elapsed_secs": time.monotonic() - started,
                                    "diagnostic_sha256": hashlib.sha256(diagnostic.stderr).hexdigest(),
                                    "seed_sha256": sha(workspace / "build-diagnostics.log"), "provider_requests": 0})
    treatment = ("Use the automatically attached build-log diagnosis if available; do not spawn a child. "
                 if lane == "candidate" else
                 "After the failure, spawn exactly one ordinary read-only child with tools=[read_file,search_files], "
                 "current route openai-codex/gpt-6.1-sol, to diagnose build-diagnostics.log. Await and integrate it. ")
    prompt = ("Run exactly `cargo check --offline --color never` once through run_command, foreground, cwd='.'. "
              "This fixture genuinely fails; its build script reproduces the supplied immutable build-diagnostics.log in its private command copy, "
              "with declared synthetic progress padding around actual compiler output. Diagnose that stored log after running Cargo. "
              "Find the primary Rust error hidden in the middle, not merely Cargo's outer build-script failure. "
              + treatment + "Do not edit sources, rerun Cargo, or start network operations. Report the compiler error code and missing symbol. "
              "Cite a precise line range and a verbatim one-line diagnostic quote from build-diagnostics.log (use targeted native search/read for citation if needed). "
              "A complete diagnosis is successful even though compilation failed. Use the final schema.")
    return prompt, {"kind": "logs", "expected_answer": ["E0425", symbol],
                    "allowed_changes": ["Cargo.lock"],
                    "target_log_bytes": size, "primary_symbol": symbol,
                    "required_commands": ["cargo check --offline --color never"]}


def fixture(run, directory):
    workspace, home = directory / "workspace", directory / "runtime"
    workspace.mkdir(parents=True)
    home.mkdir()
    runtime_wrapper(workspace)
    if run["feature"] == "narrow":
        prompt, expected = narrow_fixture(workspace, run["case"], run["lane"])
    elif run["feature"] == "context":
        prompt, expected = context_fixture(workspace, run["case"])
    elif run["feature"] == "logs":
        prompt, expected = log_fixture(workspace, run["case"], run["lane"])
    else:
        p1, e1 = narrow_fixture(workspace, 2, run["lane"])
        p2, e2 = context_fixture(workspace, 2)
        p3, e3 = log_fixture(workspace, 2, run["lane"])
        prompt = f"Complete these three phases in order.\n1. {p1}\n2. {p2}\n3. {p3}\nProduce one final schema result covering all phases and citations."
        expected = {"kind": "combined", "phases": [e1, e2, e3], "expected_answer": e2["expected_answer"] + e3["expected_answer"],
                    "allowed_changes": ["target.py", "gamma.txt", "Cargo.lock"]}
    references = {key: name for key, name in CITATION_FILES.items() if (workspace / name).is_file()}
    prompt += ("\nCitations use these fixed public source IDs in evidence.path: " + json.dumps(references)
               + ". Use the ID for the corresponding source with 1-based start_line/end_line and a verbatim quote within those exact lines. "
               "Never claim complete when required work is unverified.")
    configuration = home / ".config/ca/config.toml"
    configuration.parent.mkdir(parents=True)
    configuration.write_text(config(workspace))
    schema = directory / "output-schema.json"
    dump(schema, SCHEMA)
    (directory / "prompt.txt").write_text(prompt)
    initial = {str(path.relative_to(workspace)): sha(path) for path in workspace.rglob("*") if path.is_file()}
    dump(directory / "fixture.json", {**run, "expected": expected, "initial_sha256": initial,
                                      "padding_is_synthetic": run["feature"] in ("logs", "combined")})
    return workspace, home, prompt, expected, initial


def events(home):
    result, errors = [], []
    for path in sorted((home / ".local/share/ca/traces").glob("*.jsonl")):
        for number, line in enumerate(path.read_text(errors="replace").splitlines(), 1):
            try:
                result.append(json.loads(line))
            except json.JSONDecodeError:
                errors.append(f"{path.name}:{number}")
    return result, errors


def trace_metrics(trace, launched_at=None):
    starts = [event for event in trace if event.get("event") == "attempt_started"]
    finishes = {(event.get("request_id"), event.get("data", {}).get("attempt")): event
                for event in trace if event.get("event") == "request_finished"}
    usage = {}
    tools = []
    for event in trace:
        data = event.get("data", {})
        provider = data.get("provider_event", {})
        if event.get("event") == "decoded_provider_event":
            info = provider.get("usage")
            if isinstance(info, dict) and isinstance(info.get("prompt_tokens"), int):
                usage[event.get("request_id")] = info
            if provider.get("type") == "tool_call_start":
                tools.append(provider.get("name"))
    ids = {event.get("request_id") for event in starts}
    keys = [(event.get("request_id"), event.get("data", {}).get("attempt")) for event in starts]
    settled = bool(starts) and len(keys) == len(set(keys)) and all(key in finishes for key in keys)
    complete_usage = bool(ids) and ids <= usage.keys()
    def stamp(event):
        try:
            return datetime.fromisoformat(event["timestamp"].replace("Z", "+00:00")).timestamp()
        except (KeyError, TypeError, ValueError):
            return None
    durations = [event.get("data", {}).get("elapsed_ms") for event in finishes.values()]
    durations = [value / 1000 for value in durations if isinstance(value, (int, float))]
    first_stamp = stamp(starts[0]) if starts else None
    startup = first_stamp - launched_at if first_stamp is not None and launched_at is not None else None
    inference_ids = {event.get("request_id") for event in trace if event.get("event") == "request_started" and event.get("request_kind") == "inference"}
    inference_durations = [event.get("data", {}).get("elapsed_ms") / 1000 for event in finishes.values()
                           if event.get("request_id") in inference_ids and isinstance(event.get("data", {}).get("elapsed_ms"), (int, float))]
    return {"startup_secs": startup if startup is not None and startup >= 0 else None,
            "first_request_secs": durations[0] if durations else None,
            "warm_request_median_secs": statistics.median(durations[1:]) if len(durations) > 1 else None,
            "inference_provider_secs": sum(inference_durations) if inference_durations else None,
            "physical_calls": len(starts), "input_tokens": sum(usage[key]["prompt_tokens"] for key in ids) if complete_usage else None,
            "usage_complete": complete_usage, "trace_settled": settled,
            "failed_attempts": [event.get("data", {}) for event in finishes.values() if event.get("data", {}).get("outcome") != "completed"],
            "tools": tools, "inference_requests": sum(event.get("event") == "request_started" and event.get("request_kind") == "inference" for event in trace),
            "trace_limit_events": [event.get("event") for event in trace if any(word in event.get("event", "") for word in ("disabled", "limit", "dropped"))]}


def tool_invocations(trace, native_events=None):
    if native_events is not None:
        request_ids = {}
        for event in trace:
            provider = event.get("data", {}).get("provider_event", {})
            if event.get("event") == "decoded_provider_event" and provider.get("type") == "tool_call_start":
                key = (provider.get("id"), provider.get("name"))
                request_id = event.get("request_id")
                if all(isinstance(value, str) and value for value in (*key, request_id)):
                    request_ids.setdefault(key, set()).add(request_id)
        calls = []
        for event in native_events:
            if event.get("type") != "tool_start":
                continue
            key = (event.get("call_id"), event.get("name"))
            bound = request_ids.get(key, set())
            calls.append({"name": event.get("name"), "call_id": event.get("call_id"),
                          "request_id": next(iter(bound)) if len(bound) == 1 else None,
                          "arguments": event.get("arguments"), "evidence_source": "native_cli_tool_start"})
        return calls
    # Display traces remain available for diagnosis, but their redacted argument
    # fragments are not execution evidence for qualification surface checks.
    calls = {}
    for event in trace:
        if event.get("event") != "decoded_provider_event":
            continue
        provider = event.get("data", {}).get("provider_event", {})
        key = (event.get("request_id"), provider.get("index"))
        if provider.get("type") == "tool_call_start":
            calls[key] = {"name": provider.get("name"), "request_id": event.get("request_id"), "fragments": "", "arguments": None}
        elif provider.get("type") == "tool_call_arg" and key in calls:
            argument = provider.get("arg")
            if isinstance(argument, dict):
                calls[key]["arguments"] = argument
            elif isinstance(argument, str):
                calls[key]["fragments"] += argument
                try:
                    calls[key]["arguments"] = json.loads(calls[key]["fragments"])
                except json.JSONDecodeError:
                    pass
    return list(calls.values())


def calls_from_stdout(stdout):
    parsed = []
    for line in stdout.splitlines():
        try:
            parsed.append(json.loads(line).get("event", {}))
        except (json.JSONDecodeError, AttributeError):
            pass
    return parsed


def citations_valid(workspace, evidence):
    details = []
    if not isinstance(evidence, list) or not evidence:
        return False, []
    for citation in evidence:
        valid = False
        try:
            relative = Path(CITATION_FILES.get(citation["path"], citation["path"]))
            path = (workspace / relative).resolve()
            start, end = citation["start_line"], citation["end_line"]
            valid = (not relative.is_absolute() and path.is_relative_to(workspace.resolve())
                     and isinstance(start, int) and isinstance(end, int) and 1 <= start <= end
                     and isinstance(citation["quote"], str) and bool(citation["quote"]))
            if valid:
                lines = path.read_text().splitlines(keepends=True)
                valid = end <= len(lines) and citation["quote"] in "".join(lines[start - 1:end])
        except (KeyError, TypeError, OSError, UnicodeError):
            valid = False
        details.append({"citation": citation, "valid": valid})
    return all(item["valid"] for item in details), details


def command_log_binding(workspace, initial, cli_events):
    """Use the emitted host generation receipt, never guessed anonymous paths."""
    starts = {event["call_id"] for event in cli_events if event.get("type") == "tool_start"
              and event.get("name") == "run_command" and isinstance(event.get("arguments"), dict)
              and event["arguments"].get("command") == "cargo check --offline --color never"}
    finishes = [event for event in cli_events if event.get("type") == "tool_finish" and event.get("call_id") in starts]
    markers = {match for event in finishes for match in re.findall(
        r"command_source_generation: ([a-f0-9]{64})", event.get("output", ""))}
    home = workspace.parent / "runtime"
    candidates = []
    for receipt_path in home.rglob("generation.json"):
        if receipt_path.is_symlink() or not receipt_path.resolve().is_relative_to(home.resolve()):
            continue
        try:
            receipt = json.loads(receipt_path.read_text())
            if (receipt.get("schema") != "ca.command-workspace.v1" or receipt.get("source_digest") not in markers
                    or receipt_path.parent.parent.name != receipt.get("session_id")
                    or receipt_path.parent.parent.parent.name != "command-workspaces"):
                continue
            for root in receipt_path.parent.glob("root-*"):
                files = ("Cargo.toml", "build.rs", "broken.rs", "lib.rs")
                if all(not (root / name).is_symlink() and (root / name).is_file()
                       and sha(root / name) == initial.get(name) for name in files):
                    generated = root / "build-diagnostics.log"
                    if not generated.is_symlink() and generated.is_file() and sha(generated) == initial.get("build-diagnostics.log"):
                        candidates.append({"generation": receipt["source_digest"], "session_id": receipt["session_id"],
                                           "receipt": str(receipt_path), "generated_log": str(generated),
                                           "sha256": sha(generated)})
        except (OSError, ValueError, TypeError):
            continue
    return {"passed": len(markers) == 1 and len(candidates) == 1,
            "emitted_generations": sorted(markers), "matching_owned_artifacts": candidates}


def captured_log_citations(workspace, result):
    """Reopen immutable captured slices cited by actual inference operations."""
    quotes = [item["quote"].encode() for item in (result or {}).get("evidence", [])
              if CITATION_FILES.get(item.get("path"), item.get("path")) == "build-diagnostics.log"]
    store = workspace.parent / "runtime/.local/share/ca/inference-artifacts"
    slices = []
    try:
        for path in store.glob("*/*/trace-*.json"):
            trace = json.loads(path.read_text())
            if trace.get("phase") != "finished":
                continue
            for reference in trace.get("context_refs", []):
                directory = path.parent / reference["artifact_id"]
                manifest = json.loads((directory / "manifest.json").read_text())
                stream = reference["stream"]
                raw = (directory / stream).read_bytes()
                start, end = reference["start"], reference["end"]
                if (manifest["artifact"]["complete"] and manifest.get("outcome") == {"status": "failed", "exit_code": 101}
                        and hashlib.sha256(raw).hexdigest() == manifest[f"{stream}_sha256"]
                        and len(raw) == manifest["artifact"][f"{stream}_bytes"]
                        and 0 <= start < end <= len(raw) and end - start <= 32768):
                    slices.append(raw[start:end])
    except (OSError, ValueError, KeyError, TypeError):
        return False
    return bool(quotes) and all(any(quote in piece for piece in slices) for quote in quotes)


def score(workspace, expected, initial, result, cli_events):
    quality = {}
    changed = [name for name, digest in initial.items() if not (workspace / name).is_file() or sha(workspace / name) != digest]
    quality["protected_files_preserved"] = not (set(changed) - set(expected["allowed_changes"]))
    quality["schema_value_returned"] = isinstance(result, dict)
    quality["complete_claim"] = isinstance(result, dict) and result.get("status") == "complete"
    answer = result.get("answer", "") if isinstance(result, dict) else ""
    quality["answer_correct"] = isinstance(answer, str) and all(value in answer for value in expected["expected_answer"])
    valid, citation_details = citations_valid(workspace, result.get("evidence") if isinstance(result, dict) else None)
    quality["citations_valid"] = valid
    phases = expected.get("phases", [expected])
    for phase in phases:
        kind = phase["kind"]
        if kind == "narrow":
            try:
                verifier = workspace / "check_target.py"
                if verifier.is_symlink() or not verifier.is_file() or sha(verifier) != initial.get("check_target.py"):
                    raise ValueError("fixed acceptance verifier changed; do not execute candidate bytes")
                wrapper = Path(phase["check_command"][0])
                if (wrapper != workspace.resolve() / RUNTIME_WRAPPER or wrapper.is_symlink()
                        or not wrapper.is_file() or sha(wrapper) != initial.get(RUNTIME_WRAPPER)
                        or wrapper.stat().st_mode & 0o777 != phase["runtime_wrapper_mode"]):
                    raise ValueError("fixed interpreter wrapper changed; do not execute candidate bytes")
                checked = subprocess.run(phase["check_command"], cwd=workspace, capture_output=True, timeout=15,
                                         env={"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(workspace.parent / "runtime"), "PYTHONDONTWRITEBYTECODE": "1"})
                content = (workspace / "target.py").read_text()
                original = phase["original"].splitlines(keepends=True)
                quality["narrow_pinned_acceptance"] = checked.returncode == 0
                quality["narrow_frozen_exterior"] = content.startswith(original[0]) and content.endswith("".join(original[2:]))
                quality["narrow_target_cited"] = any(CITATION_FILES.get(item.get("path"), item.get("path")) == "target.py" and item.get("start_line") == 2 for item in (result or {}).get("evidence", []))
            except (OSError, subprocess.TimeoutExpired, UnicodeError, ValueError):
                quality["narrow_pinned_acceptance"] = False
                quality["narrow_frozen_exterior"] = False
        elif kind == "context":
            wanted = phase["expected_answer"][-1]
            old = phase["initial_gamma"]
            desired = old.replace("GAMMA317=teal", wanted)
            quality["context_current_exact_bytes"] = (workspace / "gamma.txt").read_text() == desired
            quality["context_current_sources_cited"] = {"beta.txt", "gamma.txt"} <= {CITATION_FILES.get(item.get("path"), item.get("path")) for item in (result or {}).get("evidence", [])}
        elif kind == "logs":
            log = workspace / "build-diagnostics.log"
            text = log.read_text(errors="replace") if log.is_file() else ""
            middle = text.find("error[E0425]")
            quality["real_decisive_diagnostic_in_middle"] = (log.is_file() and log.stat().st_size >= 1024 * 1024
                and 0.35 < middle / max(len(text), 1) < 0.65 and phase["primary_symbol"] in text)
            quality["decisive_log_cited"] = any(CITATION_FILES.get(item.get("path"), item.get("path")) == "build-diagnostics.log" and phase["primary_symbol"] in item.get("quote", "") for item in (result or {}).get("evidence", []))
            quality["stored_log_matches_current_command_artifact"] = command_log_binding(workspace, initial, cli_events)["passed"]
    tool_calls = [event for event in cli_events if event.get("type") == "tool_start"]
    names = [event.get("name") for event in tool_calls]
    quality["required_normal_surface_exercised"] = bool(tool_calls)
    return {"checks": quality, "citation_details": citation_details,
            "quality_passed": all(quality.values()),
            "false_completion": quality["complete_claim"] and not all(quality.values()),
            "tool_names": names, "changed_fixture_files": changed}


def stop(process):
    if process.poll() is not None:
        return
    os.killpg(process.pid, signal.SIGINT)
    try:
        process.wait(timeout=8)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()


def run_one(run, binary, directory, deadline, run_timeout, max_calls):
    workspace, home, prompt, expected, initial = fixture(run, directory)
    env = environment(home, directory)
    python_transport = python_preflight(env)
    command = [str(binary), "--no-session", "--no-keep-awake", "--yolo", "--sandbox", "required", "--trace",
               "--output-format", "jsonl", "--output-schema", str(directory / "output-schema.json"),
               "--tools", "read_file,edit_file,run_command,spawn_agent,await_agent,search_files,list_dir", "-p", prompt]
    dump(directory / "launch.json", {"argv": command, "route": "openai-codex/gpt-6.1-sol", "api_key": "empty native-auth placeholder",
                                     "python_transport": python_transport,
                                     "budgets": {"wall_secs": run_timeout, "physical_call_observation_cap": max_calls,
                                                 "parent_turns": 24, "child_turns": 12, "child_count": 2}})
    started = time.monotonic()
    launched_at = time.time()
    stop_reason = None
    with (directory / "stdout.jsonl").open("wb") as stdout, (directory / "stderr.txt").open("wb") as stderr:
        process = subprocess.Popen(command, cwd=workspace, env=env, stdout=stdout, stderr=stderr, start_new_session=True)
        while process.poll() is None:
            now = time.monotonic()
            if now >= deadline:
                stop_reason = "suite_live_wall_limit"
            elif now - started >= run_timeout:
                stop_reason = "equal_per_task_wall_limit"
            elif sum(path.read_bytes().count(b'"event":"attempt_started"') for path in (home / ".local/share/ca/traces").glob("*.jsonl")) > max_calls:
                stop_reason = "physical_call_cap_exceeded"
            if stop_reason:
                stop(process)
                break
            time.sleep(0.25)
    elapsed = time.monotonic() - started
    trace, malformed = events(home)
    metrics = trace_metrics(trace, launched_at)
    cli_events = calls_from_stdout((directory / "stdout.jsonl").read_text(errors="replace"))
    results = [event.get("value") for event in cli_events if event.get("type") == "structured_output"]
    result = results[-1] if results else None
    scored = score(workspace, expected, initial, result, cli_events)
    failures = [event for event in cli_events if event.get("type") == "failure"]
    provider_failure = any(event.get("kind") in ("provider", "provider_error", "protocol", "initialization", "io") for event in failures)
    mapping_unknown = isinstance(result, dict) and any(
        "__CA_" in str(item.get("path", "")) or "__CA_" in str(item.get("quote", ""))
        for item in result.get("evidence", []) if isinstance(item, dict))
    infrastructure = (provider_failure or bool(metrics["failed_attempts"]) or bool(malformed) or not metrics["trace_settled"]
                      or bool(metrics["trace_limit_events"]) or stop_reason == "suite_live_wall_limit" or mapping_unknown)
    invocations = tool_invocations(trace, cli_events)
    surface = [call["name"] for call in invocations]
    if run["feature"] in ("narrow", "combined"):
        spawns = [call for call in invocations if call["name"] == "spawn_agent"]
        scored["checks"]["narrow_child_surface"] = any(
            isinstance(call["arguments"], dict) and (isinstance(call["arguments"].get("repair_contract"), dict) if run["lane"] == "candidate" else call["arguments"].get("repair_contract") is None)
            for call in spawns)
    if run["feature"] in ("logs", "combined"):
        scored["checks"]["log_lane_surface"] = (metrics["inference_requests"] > 0 if run["lane"] == "candidate" else "spawn_agent" in surface)
        scored["checks"]["cargo_run_once_through_normal_entry"] = sum(call["name"] == "run_command" and isinstance(call["arguments"], dict) and call["arguments"].get("command") == "cargo check --offline --color never" for call in invocations) == 1
        scored["command_log_binding"] = command_log_binding(workspace, initial, cli_events)
        if run["lane"] == "candidate":
            scored["checks"]["diagnostic_citations_reopen_captured_inference_bytes"] = captured_log_citations(workspace, result)
    if run["feature"] in ("context", "combined"):
        reads = [call for call in invocations if call["name"] == "read_file" and isinstance(call["arguments"], dict)]
        # Combined's repair may read first; locate the mandated A/B/C/A/A/A/A
        # window without assuming that unrelated reads have particular paths.
        scored["checks"]["context_repeated_reads"] = any(
            all(call["request_id"] for call in reads[index:index + 7])
            and len({call["request_id"] for call in reads[index:index + 7]}) == 7
            and len({call["arguments"].get("path") for call in reads[index:index + 3]}) == 3
            and all(call["arguments"].get("path") == reads[index]["arguments"].get("path") for call in reads[index + 3:index + 7])
            for index in range(max(0, len(reads) - 6)))
    scored["quality_passed"] = all(scored["checks"].values())
    scored["false_completion"] = scored["checks"]["complete_claim"] and not scored["quality_passed"]
    receipt = {**run, "binary": str(binary), "binary_sha256": sha(binary), "elapsed_secs": elapsed,
               "exit_code": process.returncode, "stop_reason": stop_reason, "metrics": metrics, "score": scored,
               "failure_events": failures, "trace_parse_errors": malformed, "infrastructure_failure": infrastructure,
               "citation_mapping_unknown": mapping_unknown,
               "outcome": "INFRA_ERROR" if infrastructure else "PASS" if scored["quality_passed"] and process.returncode == 0 and not stop_reason else "FAIL"}
    dump(directory / "receipt.json", receipt)
    return receipt


def summarize(receipts):
    features = {}
    for feature in ("narrow", "context", "logs", "combined"):
        rows = [row for row in receipts if row["feature"] == feature]
        lanes = {lane: [row for row in rows if row["lane"] == lane] for lane in ("baseline", "candidate")}
        expected = 3 if feature == "combined" else 9
        complete = all(len(rows) == expected for rows in lanes.values())
        measured = complete and all(not row["infrastructure_failure"] and row["metrics"]["usage_complete"] for row in rows)
        medians = {}
        for lane, runs in lanes.items():
            medians[lane] = {key: statistics.median([row["elapsed_secs"] if key == "elapsed_secs" else row["metrics"][key] for row in runs])
                             if runs and all((row["elapsed_secs"] if key == "elapsed_secs" else row["metrics"][key]) is not None for row in runs) else None
                             for key in ("elapsed_secs", "physical_calls", "input_tokens")}
        reductions = {}
        for key in ("physical_calls", "input_tokens"):
            base, candidate = medians["baseline"][key], medians["candidate"][key]
            reductions[key] = 1 - candidate / base if base and candidate is not None else None
        base_time, candidate_time = medians["baseline"]["elapsed_secs"], medians["candidate"]["elapsed_secs"]
        time_growth = candidate_time / base_time - 1 if base_time and candidate_time is not None else None
        paired = {(row["case"], row["repetition"], row["lane"]): row for row in rows}
        no_regression = complete and all(not row["score"]["false_completion"] for row in lanes["candidate"]) and all(
            paired[(case, rep, "baseline")]["outcome"] != "PASS" or paired[(case, rep, "candidate")]["outcome"] == "PASS"
            for case, rep, lane in paired if lane == "baseline" and (case, rep, "candidate") in paired)
        # Quality must succeed, not merely tie an entirely failing baseline.
        candidate_quality = complete and all(row["outcome"] == "PASS" for row in lanes["candidate"])
        efficiency = any(value is not None and value >= 0.10 for value in reductions.values()) and time_growth is not None and time_growth <= 0.05
        features[feature] = {"complete_pairs": complete, "measurement_complete": measured, "medians": medians,
                             "reductions": reductions, "elapsed_growth": time_growth, "no_quality_regression": no_regression,
                             "all_candidate_acceptance_passed": candidate_quality, "efficiency_gate": efficiency,
                             "efficiency_required": feature != "combined",
                             "passed": bool(measured and no_regression and candidate_quality and (efficiency or feature == "combined"))}
    return {"schema": "ca.hrs317-qualification.v1", "runs_completed": len(receipts), "runs_planned": 60,
            "features": features, "passed": len(receipts) == 60 and all(item["passed"] for item in features.values()),
            "unmeasured_is_not_pass": True}


def self_test():
    assert len(plan()) == 60
    with tempfile.TemporaryDirectory(prefix="hrs317-python-transport-") as temporary:
        root = Path(temporary)
        first_env = None
        for case in range(3):
            checkers = []
            for lane in ("baseline", "candidate"):
                directory = root / f"{case}-{lane}"
                workspace = directory / "workspace"
                home = directory / "runtime"
                workspace.mkdir(parents=True)
                home.mkdir()
                prompt, expected = narrow_fixture(workspace, case, lane)
                checkers.append((workspace / "check_target.py").read_bytes())
                env = environment(home, directory)
                first_env = first_env or env
                assert Path(shutil.which(PUBLIC_PYTHON, path=env["PATH"])).resolve() == workspace.resolve() / RUNTIME_WRAPPER
                selected = expected["check_transport"]["selected_grant_command"]
                assert selected in prompt
                assert shlex.split(selected)[0] == (str(workspace.resolve() / RUNTIME_WRAPPER) if lane == "candidate" else PUBLIC_PYTHON)
                assert sys.executable not in prompt and str(Path(sys.executable).resolve()) not in prompt
                assert expected["check_transport"]["same_runtime_sha256"] == sha(Path(sys.executable).resolve())
            assert checkers[0] == checkers[1], "public transport must not alter the paired pinned assertions"
            assert (root / f"{case}-baseline/workspace" / RUNTIME_WRAPPER).read_bytes() == (root / f"{case}-candidate/workspace" / RUNTIME_WRAPPER).read_bytes()
        assert python_preflight(first_env)["runtime_sha256"] == sha(Path(sys.executable).resolve())
    native = [{"type": "tool_start", "call_id": "native-cargo", "name": "run_command",
               "arguments": {"command": "cargo check --offline --color never"}}]
    redacted = [{"event": "decoded_provider_event", "request_id": "provider-request", "data": {
        "provider_event": {"type": "tool_call_start", "id": "native-cargo", "name": "run_command", "index": 0}}},
        {"event": "decoded_provider_event", "request_id": "provider-request", "data": {
            "provider_event": {"type": "tool_call_arg", "index": 0, "arg": '{"redacted_fragment'}}}]
    actual = tool_invocations(redacted, native)
    assert actual[0]["arguments"] == native[0]["arguments"] and actual[0]["request_id"] == "provider-request"
    assert tool_invocations(redacted, []) == [], "proposed trace calls are not native operation evidence"
    assert tool_invocations(redacted, [{**native[0], "name": "different_tool"}])[0]["request_id"] is None
    ambiguous = redacted + [{"event": "decoded_provider_event", "request_id": "other-request", "data": {
        "provider_event": {"type": "tool_call_start", "id": "native-cargo", "name": "run_command", "index": 0}}}]
    assert tool_invocations(ambiguous, native)[0]["request_id"] is None, "ambiguous turns cannot qualify as distinct"
    assert all(sum(run["feature"] == feature for run in plan()) == count for feature, count in (("narrow", 18), ("context", 18), ("logs", 18), ("combined", 6)))
    with tempfile.TemporaryDirectory(prefix="hrs317-contract-") as temporary:
        workspace = Path(temporary)
        (workspace / "evidence.txt").write_text("first\ncorrect fact\nlast\n")
        assert citations_valid(workspace, [{"path": "evidence.txt", "start_line": 2, "end_line": 2, "quote": "correct fact"}])[0]
        assert not citations_valid(workspace, [{"path": "evidence.txt", "start_line": 1, "end_line": 1, "quote": "correct fact"}])[0]
        assert not citations_valid(workspace, [{"path": "../outside", "start_line": 1, "end_line": 1, "quote": "secret"}])[0]
        workspace = workspace / "binding/workspace"
        workspace.mkdir(parents=True)
        home = workspace.parent / "runtime"
        session = "123e4567-e89b-42d3-a456-426614174000"
        generation = "a" * 64
        receipt_path = home / "command-workspaces" / session / "generation-test/generation.json"
        artifact = receipt_path.parent / "root-0"
        artifact.mkdir(parents=True)
        initial = {}
        for name in ("Cargo.toml", "build.rs", "broken.rs", "lib.rs", "build-diagnostics.log"):
            (workspace / name).write_text(name + "\n")
            shutil.copy2(workspace / name, artifact / name)
            initial[name] = sha(workspace / name)
        dump(receipt_path, {"schema": "ca.command-workspace.v1", "session_id": session, "source_digest": generation})
        cli = [{"type": "tool_start", "call_id": "cargo", "name": "run_command", "arguments": {"command": "cargo check --offline --color never"}},
               {"type": "tool_finish", "call_id": "cargo", "output": "command_source_generation: " + generation}]
        assert command_log_binding(workspace, initial, cli)["passed"]
        (artifact / "build-diagnostics.log").write_text("different command output")
        assert not command_log_binding(workspace, initial, cli)["passed"]
        shutil.copy2(workspace / "build-diagnostics.log", artifact / "build-diagnostics.log")
        dump(receipt_path, {"schema": "ca.command-workspace.v1", "session_id": "other", "source_digest": generation})
        assert not command_log_binding(workspace, initial, cli)["passed"]
        owner = home / ".local/share/ca/inference-artifacts/session/run"
        capture = owner / "artifact-test"
        capture.mkdir(parents=True)
        raw = b"before\nerror[E0425]: missing_symbol317_0\nafter\n"
        (capture / "stderr").write_bytes(raw)
        dump(capture / "manifest.json", {"artifact": {"complete": True, "stderr_bytes": len(raw)},
                                         "stderr_sha256": hashlib.sha256(raw).hexdigest(),
                                         "outcome": {"status": "failed", "exit_code": 101}})
        dump(owner / "trace-test.json", {"phase": "finished", "context_refs": [{"artifact_id": "artifact-test", "stream": "stderr", "start": 7, "end": len(raw)}]})
        value = {"evidence": [{"path": "HRS317_E4", "quote": "error[E0425]: missing_symbol317_0"}]}
        assert captured_log_citations(workspace, value)
        (capture / "stderr").write_bytes(raw.replace(b"missing", b"changed"))
        assert not captured_log_citations(workspace, value)
    assert not summarize([])["passed"]
    rows = [{**run, "outcome": "PASS", "infrastructure_failure": False, "elapsed_secs": 1,
             "metrics": {"usage_complete": True, "physical_calls": 10 if run["lane"] == "baseline" else 8,
                         "input_tokens": 100 if run["lane"] == "baseline" else 80},
             "score": {"quality_passed": True, "false_completion": False}} for run in plan()]
    assert summarize(rows)["passed"]
    next(row for row in rows if row["lane"] == "candidate")["outcome"] = "FAIL"
    assert not summarize(rows)["passed"], "valid output cannot override a stopped or nonzero-exit run"
    synthetic = [{"event": "attempt_started", "request_id": "a", "data": {"attempt": 1}},
                 {"event": "request_finished", "request_id": "a", "data": {"attempt": 1, "outcome": "completed"}},
                 {"event": "decoded_provider_event", "request_id": "a", "data": {"provider_event": {"type": "usage", "usage": {"prompt_tokens": 12}}}},
                 {"event": "decoded_provider_event", "request_id": "a", "data": {"provider_event": {"type": "finish", "usage": {"prompt_tokens": 12}}}}]
    measured = trace_metrics(synthetic)
    assert measured["physical_calls"] == 1 and measured["input_tokens"] == 12
    print(json.dumps({"driver_contracts": "PASS", "provider_requests": 0}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, default=DEFAULT_BASELINE)
    parser.add_argument("--candidate", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--prepare-only", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--live-seconds", type=int, default=3600)
    parser.add_argument("--run-seconds", type=int, default=300)
    parser.add_argument("--max-calls", type=int, default=48)
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    if args.candidate is None or args.output is None:
        parser.error("--candidate and --output are required")
    if not 1 <= args.live_seconds <= 3600 or not 1 <= args.run_seconds <= 300 or not 1 <= args.max_calls <= 48:
        parser.error("live cap must be <=3600 seconds, task cap <=300 seconds and call cap <=48")
    baseline, candidate = args.baseline.resolve(), args.candidate.resolve()
    for binary in (baseline, candidate):
        if not binary.is_file() or not os.access(binary, os.X_OK):
            parser.error(f"executable unavailable: {binary}")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    manifest = {"schema": "ca.hrs317-qualification-plan.v1", "runs": plan(), "baseline": str(baseline),
                "baseline_sha256": sha(baseline), "candidate": str(candidate), "candidate_sha256": sha(candidate),
                "route": "openai-codex/gpt-6.1-sol", "live_seconds_cap": args.live_seconds,
                "task_seconds_cap": args.run_seconds, "physical_calls_cap": args.max_calls,
                "padding": "declared synthetic progress around real rustc failure inside actual Cargo build script",
                "held_out_policy": "case 2 per feature plus combined; no fixture or scorer adjustment based on live outcomes"}
    dump(output / "plan.json", manifest)
    if args.prepare_only:
        for run in plan():
            fixture(run, output / run["id"])
        dump(output / "receipt.json", {"status": "PREPARED_NOT_RUN", "provider_requests": 0, "planned_runs": 60, "passed": False})
        print(json.dumps({"status": "PREPARED_NOT_RUN", "output": str(output), "provider_requests": 0}))
        return 0
    started = time.monotonic()
    deadline = started + args.live_seconds
    receipts = []
    for run in plan():
        if time.monotonic() >= deadline:
            break
        receipt = run_one(run, baseline if run["lane"] == "baseline" else candidate,
                          output / run["id"], deadline, args.run_seconds, args.max_calls)
        receipts.append(receipt)
        summary = summarize(receipts)
        summary["elapsed_secs"] = time.monotonic() - started
        dump(output / "receipt.json", summary)
        print(json.dumps({"run": run["id"], "outcome": receipt["outcome"], "completed": len(receipts),
                          "seconds_remaining": max(0, round(deadline - time.monotonic(), 1))}), flush=True)
    summary = summarize(receipts)
    summary["elapsed_secs"] = time.monotonic() - started
    summary["live_cap_exhausted"] = len(receipts) != 60 and time.monotonic() >= deadline
    summary["unstarted_runs"] = [run["id"] for run in plan()[len(receipts):]]
    summary["artifacts_preserved_at"] = str(output)
    dump(output / "receipt.json", summary)
    print(json.dumps({"passed": summary["passed"], "receipt": str(output / "receipt.json")}))
    return 0 if summary["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
