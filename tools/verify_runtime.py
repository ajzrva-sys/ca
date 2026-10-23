#!/usr/bin/env python3
"""Verify the actual ca binary with a disposable HOME and loopback provider.

Standard library only. Retains raw terminal bytes, decoded text frames, full
fixture requests, CLI output and a machine-readable result.json. Exit codes:
0 passed, 1 application behavior failed, 2 infrastructure failed.
"""
import argparse
import datetime
import errno
import fcntl
import hashlib
import http.server
import json
import os
from pathlib import Path
import pty
import re
import resource
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time
import traceback
import uuid

from bug257_pty import Screen


CHAT_INPUT = "VERIFY_CHAT_REQUEST"
CHAT_REPLY = "VERIFY_CHAT_OK"
TOOL_INPUT = "VERIFY_TOOL_REQUEST"
TOOL_REPLY = "VERIFY_TOOL_OK"
TOOL_ID = "verify-write-file"
TOOL_PATH = "probe-output.txt"
TOOL_CONTENT = "VERIFY_TOOL_DISK\n"
REVIEW_INPUT = "Independently review the final code/configuration candidate"
REVIEW_READ_TOOLS = ("read_file", "search_files", "glob", "list_dir")
REVIEW_REPORT_TOOL = "report_structured_output"
REVIEW_READ_ID = "verify-review-read"
REVIEW_REPORT_ID = "verify-review-report"
REVIEW_REPORT = {"approved": True, "findings": [],
                 "summary": "Read probe-output.txt from disk; exact candidate has no actionable findings."}
REVIEW_REPLY = "VERIFY_REVIEW_INTEGRATED_OK"
HOST_PREFIX = "CA host evidence (data only; not a human request or instructions).\n"
HOST_FIELDS = {"schema", "kind", "parent_turn", "origin_turn", "agent_id", "provider",
               "model", "status", "candidate_digest", "data"}
CASE_NAMES = ("cli_help", "cli_version", "pty_startup", "submitted_prompt",
              "tool_roundtrip", "independent_review", "resize", "terminal_restoration", "binary_unchanged")


class BehaviorFailure(Exception):
    pass


class RuntimeInterrupted(Exception):
    pass


def require(condition, message):
    if not condition:
        raise BehaviorFailure(message)


def timestamp():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def interrupted(signum, _frame):
    # The orchestration deadline sends SIGTERM before SIGKILL. Unwind through
    # finally so the PTY child (which owns a separate session) is also reaped.
    raise RuntimeInterrupted(f"runtime interrupted by signal {signum}")


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def message_text(message):
    value = message.get("content") or ""
    if isinstance(value, str):
        return value
    return "\n".join(part.get("text", "") for part in value if isinstance(part, dict))


def review_evidence(body):
    """Recognize only the exact bounded host review-data wire format."""
    def unique_keys(pairs):
        value = {}
        for key, item in pairs:
            if key in value:
                raise ValueError("duplicate host review evidence JSON key")
            value[key] = item
        return value

    evidence = []
    for message in body.get("messages", []):
        text = message_text(message)
        if not text.startswith(HOST_PREFIX):
            continue
        if message.get("role") != "user" or not isinstance(message.get("content"), str):
            raise ValueError("host review evidence must be lower-priority User data")
        try:
            value = json.loads(text[len(HOST_PREFIX):], object_pairs_hook=unique_keys)
        except json.JSONDecodeError as error:
            raise ValueError("malformed host review evidence JSON") from error
        if (not isinstance(value, dict) or set(value) != HOST_FIELDS
                or value.get("schema") != "ca.host-evidence.v1"
                or value.get("kind") != "independent_review"):
            raise ValueError("unexpected host review evidence schema/kind/fields")
        try:
            turn = uuid.UUID(value.get("parent_turn"))
        except (ValueError, TypeError, AttributeError) as error:
            raise ValueError("host review evidence current-turn binding is invalid") from error
        if (str(turn) != value.get("parent_turn") or turn.int == 0
                or value.get("origin_turn") != str(turn)):
            raise ValueError("host review evidence current/origin turn differs")
        if (any(value.get(key) is not None for key in ("agent_id", "provider", "model"))
                or value.get("status") != "Approved"
                or not isinstance(value.get("candidate_digest"), str)
                or re.fullmatch(r"[0-9a-f]{64}", value["candidate_digest"]) is None):
            raise ValueError("host review evidence identity/route/status/candidate differs")
        data = value.get("data")
        if (not isinstance(data, dict) or set(data) != set(REVIEW_REPORT)
                or data.get("approved") is not True or data != REVIEW_REPORT):
            raise ValueError("host review evidence does not match the actual approval report")
        evidence.append(value)
    if len(evidence) > 1:
        raise ValueError("host review evidence published more than once")
    return evidence[0] if evidence else None


def request_user(body):
    evidence = review_evidence(body)
    users = [message_text(message) for message in body.get("messages", [])
             if message.get("role") == "user" and not message_text(message).startswith(HOST_PREFIX)]
    if evidence and users != [CHAT_INPUT, TOOL_INPUT]:
        raise ValueError("host review evidence changed the original human request")
    return users[-1] if users else ""


def turn_owned_candidate_digest(workspace, turn):
    """Independently bind the one known Missing→UTF8 fixture edit to its turn."""
    workspace = workspace.resolve()
    target = workspace / TOOL_PATH
    value = hashlib.sha256()
    value.update(b"ca.automatic-review.v1:independent-readonly-exact-candidate")
    value.update(uuid.UUID(turn).bytes)
    value.update(str(uuid.uuid5(uuid.NAMESPACE_URL, str(workspace))).encode())
    value.update(os.fsencode(workspace))
    value.update(os.fsencode(target))
    value.update(b"missing\0present\0")
    value.update(hashlib.sha256(TOOL_CONTENT.encode()).digest())
    return value.hexdigest()


def function_call(identifier, name, arguments):
    return {"index": 0, "id": identifier, "type": "function",
            "function": {"name": name, "arguments": json.dumps(arguments)}}


def review_candidate_digest(body, workspace):
    context = "\n".join(message_text(message) for message in body.get("messages", [])
                        if message.get("role") in ("system", "developer"))
    digest = re.search(r"Independently review the exact turn-owned candidate ([0-9a-f]{64})\.", context)
    inventory = context.split("Exact changed-file inventory:\n", 1)
    target = workspace.resolve() / TOOL_PATH
    if (not digest or len(inventory) != 2
            or inventory[1].split("\n\n", 1)[0].splitlines() != [str(target)]
            or "Original request (context only):\n" + TOOL_INPUT + "\n" not in context
            or "Changed path: " + TOOL_PATH + "\nBefore:\n[missing]\nCandidate:\n" + TOOL_CONTENT not in context):
        raise ValueError("independent review changed-file context did not match the real write")
    if target.is_symlink() or not target.is_file() or target.read_text() != TOOL_CONTENT:
        raise ValueError("independent review on-disk candidate did not match the real write")
    return digest.group(1)


def fixture_response(body, workspace, prior_requests=()):
    """Serve only the original two flows and the exact bounded reviewer flow."""
    messages = body.get("messages", [])
    user = request_user(body)
    evidence = review_evidence(body)
    if evidence:
        if (body.get("model") != "fixture-model" or body.get("stream") is not True
                or len(prior_requests) != 5
                or [request_user(entry.get("body", {})) for entry in prior_requests]
                != [CHAT_INPUT, TOOL_INPUT, TOOL_INPUT, REVIEW_INPUT, REVIEW_INPUT]
                or any(review_evidence(entry.get("body", {})) for entry in prior_requests)
                or any(entry.get("path") != "/v1/chat/completions"
                       or entry.get("body", {}).get("model") != "fixture-model"
                       or not entry.get("response_completed_at") for entry in prior_requests)):
            raise ValueError("host review evidence preceded the actual served reviewer roundtrip")
        reviews = prior_requests[-2:]
        digests = [review_candidate_digest(entry["body"], workspace) for entry in reviews]
        if (digests != [evidence["candidate_digest"]] * 2
                or turn_owned_candidate_digest(workspace, evidence["parent_turn"]) != digests[0]):
            raise ValueError("host review evidence is not the exact current-turn candidate")
        if (reviews[0].get("response_tool_calls") != [function_call(REVIEW_READ_ID, "read_file", {"path": TOOL_PATH})]
                or reviews[1].get("response_tool_calls") != [function_call(REVIEW_REPORT_ID, REVIEW_REPORT_TOOL, {"value": REVIEW_REPORT})]
                or fixture_response(reviews[0]["body"], workspace)[0] != reviews[0]["response_tool_calls"]
                or fixture_response(reviews[1]["body"], workspace)[0] != reviews[1]["response_tool_calls"]):
            raise ValueError("host review evidence lacks the actual readback and sole approval report")
        return [], REVIEW_REPLY
    if user == CHAT_INPUT:
        return [], CHAT_REPLY
    if user == TOOL_INPUT:
        results = [message for message in messages if message.get("role") == "tool"
                   and message.get("tool_call_id") == TOOL_ID]
        return ([], TOOL_REPLY) if results else (
            [function_call(TOOL_ID, "write_file", {"path": TOOL_PATH, "content": TOOL_CONTENT})], "")
    if user != REVIEW_INPUT:
        raise ValueError("unexpected user input: " + repr(user[:200]))
    if [message_text(message) for message in messages if message.get("role") == "user"] != [REVIEW_INPUT]:
        raise ValueError("independent reviewer original human input differs")
    definitions = body.get("tools", [])
    names = [tool.get("function", {}).get("name") for tool in definitions]
    if len(names) != 5 or set(names) != set(REVIEW_READ_TOOLS) | {REVIEW_REPORT_TOOL}:
        raise ValueError("independent reviewer must have exactly the four read-only review tools and report")
    report_definition = next(tool["function"] for tool in definitions
                             if tool.get("function", {}).get("name") == REVIEW_REPORT_TOOL)
    parameters = report_definition.get("parameters", {})
    value = parameters.get("properties", {}).get("value", {})
    fields = value.get("properties", {})
    if (parameters.get("type") != "object" or parameters.get("additionalProperties") is not False
            or parameters.get("required") != ["value"] or set(parameters.get("properties", {})) != {"value"}
            or value.get("type") != "object" or value.get("additionalProperties") is not False
            or set(value.get("required", [])) != {"approved", "findings", "summary"}
            or set(fields) != {"approved", "findings", "summary"}
            or fields.get("approved", {}).get("type") != "boolean"
            or fields.get("findings", {}).get("type") != "array"
            or fields.get("summary", {}).get("type") != "string"):
        raise ValueError("independent reviewer report envelope did not require the validated value object")
    review_candidate_digest(body, workspace)
    reads = [message for message in messages if message.get("role") == "tool"
             and message.get("tool_call_id") == REVIEW_READ_ID]
    if not reads:
        return [function_call(REVIEW_READ_ID, "read_file", {"path": TOOL_PATH})], ""
    matching_calls = [call for message in messages if message.get("role") == "assistant"
                      for call in message.get("tool_calls", []) if call.get("id") == REVIEW_READ_ID]
    if (len(reads) != 1 or len(matching_calls) != 1
            or matching_calls[0].get("function", {}).get("name") != "read_file"
            or json.loads(matching_calls[0].get("function", {}).get("arguments", "null")) != {"path": TOOL_PATH}
            or TOOL_CONTENT.strip() not in message_text(reads[0])):
        raise ValueError("independent reviewer read result did not prove the requested real file read")
    return [function_call(REVIEW_REPORT_ID, REVIEW_REPORT_TOOL, {"value": dict(REVIEW_REPORT)})], ""


def review_finalization_offset(raw, rows, after_offset):
    """Require a real bottom-status Done update after the final report response.

    Background status may replace Done before one read finishes decoding the
    chunk, so retained VT updates supply the ordered finalization evidence.
    """
    status = re.compile(rb"\x1b\[" + str(rows).encode() +
                        rb";1H(?:\x1b\[[0-?]*[ -/]*[@-~])*Done(?=\x1b|\s|$)")
    match = status.search(raw, after_offset)
    return match.start() if match else None


class Provider:
    """Strict OpenAI-compatible fixture; no external service or credentials."""
    def __init__(self, output, workspace):
        self.output = output
        self.workspace = workspace
        self.requests = []
        self.errors = []
        self.lock = threading.Lock()
        provider = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_GET(self):
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps({"object": "list", "data": [
                    {"id": "fixture-model", "object": "model"}]}).encode())

            def do_POST(self):
                try:
                    length = int(self.headers.get("Content-Length", "0"))
                    if not 0 < length <= 2 * 1024 * 1024:
                        raise ValueError("fixture request length outside bound")
                    body = json.loads(self.rfile.read(length))
                    with provider.lock:
                        prior_requests = list(provider.requests)
                        entry = {"path": self.path, "body": body, "received_at": timestamp(),
                                 "response_tool_calls": []}
                        provider.requests.append(entry)
                        provider.save()
                    if len(provider.requests) > 8:
                        raise ValueError("fixture request ceiling exceeded")
                    if not body.get("stream"):
                        raise ValueError("interactive request did not use streaming")
                    calls, text = fixture_response(body, provider.workspace, prior_requests)
                    with provider.lock:
                        entry["response_tool_calls"] = calls
                        entry["response_content"] = text
                        terminal_path = provider.output / "terminal.bin"
                        entry["response_terminal_offset"] = terminal_path.stat().st_size if terminal_path.exists() else 0
                        provider.save()
                    delta = {"role": "assistant", "tool_calls": calls} if calls else {
                        "role": "assistant", "content": text}
                    chunks = [{"id": "verify-fixture", "object": "chat.completion.chunk",
                               "model": "fixture-model", "choices": [
                                   {"index": 0, "delta": delta, "finish_reason": None}]},
                              {"id": "verify-fixture", "object": "chat.completion.chunk",
                               "model": "fixture-model", "choices": [{"index": 0,
                                   "delta": {}, "finish_reason": "tool_calls" if calls else "stop"}],
                               "usage": {"prompt_tokens": 100, "completion_tokens": 20,
                                         "total_tokens": 120}}]
                    # A subsequent parent request cannot see a response receipt
                    # until its small loopback stream has actually been flushed.
                    with provider.lock:
                        self.send_response(200)
                        self.send_header("Content-Type", "text/event-stream")
                        self.end_headers()
                        for chunk in chunks:
                            self.wfile.write(("data: " + json.dumps(chunk) + "\n\n").encode())
                        self.wfile.write(b"data: [DONE]\n\n")
                        self.wfile.flush()
                        entry["response_completed_at"] = timestamp()
                        provider.save()
                except (BrokenPipeError, ConnectionResetError):
                    pass
                except Exception as error:
                    with provider.lock:
                        provider.errors.append(str(error))
                        provider.save()
                    self.send_error(400, "fixture protocol failure")

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}/v1"
        self.save()

    def save(self):
        (self.output / "provider-requests.json").write_text(json.dumps(
            {"requests": self.requests, "protocol_errors": self.errors}, indent=2) + "\n")

    def snapshot(self):
        with self.lock:
            return list(self.requests), list(self.errors)

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)


class Terminal:
    def __init__(self, binary, workspace, environment, output):
        self.screen = Screen(120, 36)
        self.master, self.slave = pty.openpty()
        self.original_mode = termios.tcgetattr(self.slave)
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 120, 0, 0))
        self.raw = (output / "terminal.bin").open("wb")
        self.frames = output / "frames"
        self.frames.mkdir()
        self.bytes = 0
        self.output_tail = b""
        self.exit_code = None
        self.closed = False

        def session():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        try:
            self.process = subprocess.Popen([str(binary), "--profile", "default", "--yolo",
                "--exclude-workspace-context", "--new-session-id", str(uuid.uuid4()),
                "--name", "Disposable verification"], cwd=workspace, env=environment,
                stdin=self.slave, stdout=self.slave, stderr=self.slave, preexec_fn=session)
        except BaseException:
            self.raw.close()
            os.close(self.master)
            os.close(self.slave)
            raise

    def read(self, timeout=0.05):
        if select.select([self.master], [], [], timeout)[0]:
            try:
                data = os.read(self.master, 65536)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
                data = b""
            if data:
                self.raw.write(data)
                self.raw.flush()
                self.bytes += len(data)
                require(self.bytes <= 16 * 1024 * 1024, "terminal output exceeded 16 MiB bound")
                self.output_tail = (self.output_tail + data)[-65536:]
                self.screen.feed(data)

    def wait(self, predicate, description, timeout=15):
        deadline = time.monotonic() + timeout
        while not predicate():
            self.read()
            require(self.process.poll() is None,
                    f"ca exited {self.process.returncode} while waiting for {description}")
            require(time.monotonic() < deadline, f"deadline exceeded waiting for {description}")

    def wait_text(self, marker, timeout=15):
        self.wait(lambda: marker in self.screen.text(), repr(marker), timeout)

    def frame(self, name):
        path = self.frames / (name + ".txt")
        path.write_text(self.screen.text() + "\n")
        return str(path)

    def write(self, data):
        os.write(self.master, data)

    def resize(self, width, height):
        self.screen.resize(width, height)
        fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
        os.kill(self.process.pid, signal.SIGWINCH)
        self.wait_text("fixture-model")

    def stop(self):
        if self.closed:
            return
        forced = False
        try:
            if self.process.poll() is None:
                self.write(b"\x03")
                deadline = time.monotonic() + 5
                while self.process.poll() is None and time.monotonic() < deadline:
                    self.read()
            if self.process.poll() is None:
                forced = True
                os.killpg(self.process.pid, signal.SIGTERM)
                try:
                    self.process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    os.killpg(self.process.pid, signal.SIGKILL)
            self.exit_code = self.process.wait(timeout=2)
            # Keep the slave open until we inspect actual restored terminal attributes.
            for _ in range(3):
                self.read(0.02)
            # On macOS the slave becomes ENOTTY after its controlling session
            # exits; the master still exposes the same terminal attributes.
            self.restored_mode = termios.tcgetattr(self.master)
        except BaseException:
            if self.process.poll() is None:
                try:
                    os.killpg(self.process.pid, signal.SIGTERM)
                    self.process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    os.killpg(self.process.pid, signal.SIGKILL)
                    self.process.wait(timeout=2)
                except ProcessLookupError:
                    self.process.wait(timeout=2)
            self.exit_code = self.process.returncode
            raise
        finally:
            self.frame("final")
            self.closed = True
            self.raw.close()
            os.close(self.master)
            os.close(self.slave)
        require(not forced, "ca required forced termination after Ctrl-C")
        require(self.exit_code == 0, f"ca did not exit cleanly: {self.exit_code}")
        require(b"\x1b[?1049l" in self.output_tail, "alternate screen was not restored")
        require(b"\x1b[?25h" in self.output_tail, "terminal cursor was not restored")
        require(self.original_mode == self.restored_mode, "terminal attributes were not restored")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    result = {"schema": "ca.verify.runtime.v1", "status": "running", "started_at": timestamp(),
              "binary": str(args.binary.resolve()), "cases": [], "metrics": {},
              "artifacts": {"root": str(output), "requests": str(output / "provider-requests.json"),
                            "terminal": str(output / "terminal.bin"), "frames": str(output / "frames")}}
    started = time.monotonic()
    terminal = provider = None
    previous_sigterm = signal.signal(signal.SIGTERM, interrupted)

    def case(name, operation):
        began = time.monotonic()
        entry = {"name": name, "status": "passed"}
        result["cases"].append(entry)
        try:
            operation()
        except RuntimeInterrupted as error:
            entry.update(status="infrastructure_failed", error=str(error))
            raise
        except (BehaviorFailure, AssertionError) as error:
            entry.update(status="failed", error=str(error))
        except Exception as error:
            entry.update(status="infrastructure_failed", error=str(error))
            (output / (name + "-traceback.txt")).write_text(traceback.format_exc())
        finally:
            entry["duration_ms"] = round((time.monotonic() - began) * 1000, 3)
            if terminal and not terminal.closed:
                entry["frame"] = terminal.frame(name)
        return entry["status"] == "passed"

    try:
        output.mkdir(parents=True, exist_ok=True)
        os.chmod(output, 0o700)
        binary = args.binary.resolve(strict=True)
        require(binary.is_file() and os.access(binary, os.X_OK), "binary is not executable")
        result["binary_sha256"] = sha256(binary)
        # Outside the checkout: ca discovers enclosing Git roots, and long
        # profile paths can exceed the platform's Unix socket path limit.
        temporary_parent = "/private/tmp" if Path("/private/tmp").is_dir() else None
        root = Path(tempfile.mkdtemp(prefix="ca-verify-", dir=temporary_parent)).resolve()
        home, workspace = root / "home", root / "workspace"
        config = home / ".config/ca"
        config.mkdir(parents=True)
        workspace.mkdir()
        (root / "tmp").mkdir()
        # Construct from scratch: credentials, provider settings, proxy variables,
        # shell startup variables and real XDG directories are never inherited.
        environment = {"PATH": "/usr/bin:/bin", "HOME": str(home), "TERM": "xterm-256color",
            "LANG": "en_US.UTF-8", "COLORFGBG": "15;0", "SHELL": "/bin/sh",
            "XDG_CONFIG_HOME": str(home / ".config"), "XDG_DATA_HOME": str(home / ".local/share"),
            "XDG_CACHE_HOME": str(home / ".cache"), "XDG_STATE_HOME": str(home / ".local/state"),
            "TMPDIR": str(root / "tmp"), "ZDOTDIR": str(home)}
        result["isolation"] = {"home": str(home), "workspace": str(workspace),
                               "environment_keys": sorted(environment), "provider": "loopback fixture only"}
        provider = Provider(output, workspace)
        (config / "config.toml").write_text("trusted_workspaces = [" + json.dumps(str(workspace)) + "]\n"
            "[providers.fixture]\nbase_url=" + json.dumps(provider.url) + "\napi_key=''\nmodels=['fixture-model']\n"
            "[defaults]\nprovider='fixture'\nmodel='fixture-model'\n"
            "[harness_improvement]\nenabled=false\n[learning.dream]\nenabled=false\n"
            "[learning.distill]\nenabled=false\n")

        def cli(flag, pattern):
            try:
                completed = subprocess.run([str(binary), flag], cwd=workspace, env=environment,
                                           capture_output=True, timeout=10)
            except subprocess.TimeoutExpired as error:
                (output / (flag[2:] + ".stdout")).write_bytes(error.stdout or b"")
                (output / (flag[2:] + ".stderr")).write_bytes(error.stderr or b"")
                raise BehaviorFailure(flag + " exceeded 10 second deadline") from error
            (output / (flag[2:] + ".stdout")).write_bytes(completed.stdout)
            (output / (flag[2:] + ".stderr")).write_bytes(completed.stderr)
            require(completed.returncode == 0, f"{flag} exited {completed.returncode}")
            require(re.search(pattern, completed.stdout.decode("utf-8", "replace")),
                    f"{flag} output did not match expected CLI contract")

        case("cli_help", lambda: cli("--help", r"Usage:.*ca"))
        case("cli_version", lambda: cli("--version", r"^ca \d+\.\d+\.\d+"))

        def startup():
            nonlocal terminal
            terminal = Terminal(binary, workspace, environment, output)
            result["child_pid"] = terminal.process.pid
            terminal.wait_text("fixture-model", 20)
            require(CHAT_REPLY not in terminal.screen.text(), "reply marker present before submission")

        ready = case("pty_startup", startup)

        def submit(text, reply):
            began = time.monotonic()
            terminal.write(text.encode())
            terminal.wait_text(text, 5)
            input_ms = round((time.monotonic() - began) * 1000, 3)
            result["metrics"]["input_frame_ms"] = max(result["metrics"].get("input_frame_ms", 0), input_ms)
            terminal.frame(text + "-draft")
            began = time.monotonic()
            terminal.write(b"\r")
            terminal.wait_text(reply)
            result["metrics"]["reply_ms" if text == CHAT_INPUT else "tool_ms"] = round(
                (time.monotonic() - began) * 1000, 3)
            requests, errors = provider.snapshot()
            require(not errors, "fixture protocol errors: " + repr(errors))
            require(all(request["path"] == "/v1/chat/completions" and
                        request["body"].get("model") == "fixture-model" for request in requests),
                    "provider request did not use the configured loopback model/endpoint")
            return [request["body"] for request in requests
                    if request_user(request["body"]) == text and review_evidence(request["body"]) is None]

        def prompt():
            requests = submit(CHAT_INPUT, CHAT_REPLY)
            require(len(requests) == 1, f"expected one submitted chat request, observed {len(requests)}")

        def tool():
            require(not (workspace / TOOL_PATH).exists(), "tool target existed before execution")
            requests = submit(TOOL_INPUT, TOOL_REPLY)
            require(len(requests) == 2, f"expected tool call and result roundtrip, observed {len(requests)} requests")
            names = [t.get("function", {}).get("name") for t in requests[0].get("tools", [])]
            require("write_file" in names, "write_file was not advertised in the submitted request")
            target = workspace / TOOL_PATH
            require(target.is_file(), "real write_file tool did not create its on-disk output")
            require(target.read_text() == TOOL_CONTENT, "on-disk tool output did not match requested content")
            tool_messages = [m for m in requests[-1].get("messages", []) if m.get("role") == "tool"
                             and m.get("tool_call_id") == TOOL_ID]
            require(len(tool_messages) == 1, "matching tool result was not returned to the provider")
            require(TOOL_CONTENT.strip() in message_text(tool_messages[0]),
                    "provider tool result lacked the actual written content")
            result["artifacts"]["tool_output"] = str(target)
            (output / "tool-output.txt").write_bytes(target.read_bytes())

        if ready:
            ready = case("submitted_prompt", prompt)
        if ready:
            ready = case("tool_roundtrip", tool)
        if ready:
            def independent_review():
                proof = {}
                def completed():
                    requests, errors = provider.snapshot()
                    require(not errors, "fixture protocol errors: " + repr(errors))
                    reviews = [request for request in requests if request_user(request["body"]) == REVIEW_INPUT]
                    parents = [request for request in requests if review_evidence(request["body"]) is not None]
                    if (len(reviews) != 2 or len(parents) != 1
                            or "response_terminal_offset" not in reviews[-1]
                            or "response_terminal_offset" not in parents[0]
                            or not parents[0].get("response_completed_at")):
                        return False
                    require(parents[0].get("response_tool_calls") == []
                            and parents[0].get("response_content") == REVIEW_REPLY,
                            "final parent did not respond with the integrated review data")
                    raw = (output / "terminal.bin").read_bytes()
                    offset = review_finalization_offset(raw, terminal.screen.height,
                        max(reviews[-1]["response_terminal_offset"], parents[0]["response_terminal_offset"]))
                    if offset is None or not any("Reviewer" in line and "Completed" in line
                                                  for line in terminal.screen.text().splitlines()) \
                            or REVIEW_REPLY not in terminal.screen.text():
                        return False
                    proof.update(status_row=terminal.screen.height,
                                 report_response_offset=reviews[-1]["response_terminal_offset"],
                                 parent_response_offset=parents[0]["response_terminal_offset"],
                                 done_offset=offset, terminal=str(output / "terminal.bin"),
                                 completed_frame=str(terminal.frames / "independent_review.txt"))
                    return True
                terminal.wait(completed, "completed Reviewer and finalized parent", 20)
                requests, errors = provider.snapshot()
                require(not errors, "fixture protocol errors: " + repr(errors))
                reviews = [request for request in requests if request_user(request["body"]) == REVIEW_INPUT]
                parents = [request for request in requests if review_evidence(request["body"]) is not None]
                require(len(reviews) == 2, f"expected real reviewer read/report roundtrip, observed {len(reviews)} requests")
                require(len(parents) == 1 and requests[-1] is parents[0],
                        "expected one final parent evidence continuation after the reviewer")
                digests = [review_candidate_digest(request["body"], workspace) for request in reviews]
                require(digests[0] == digests[1], "review candidate changed between read and report")
                require(reviews[0]["response_tool_calls"] == [function_call(REVIEW_READ_ID, "read_file", {"path": TOOL_PATH})],
                        "reviewer did not request the real changed file")
                require(reviews[1]["response_tool_calls"] == [function_call(REVIEW_REPORT_ID, REVIEW_REPORT_TOOL, {"value": REVIEW_REPORT})],
                        "reviewer did not return a sole value-enveloped report")
                result["independent_review"] = {
                    "request_count": 2, "tool_names": sorted((*REVIEW_READ_TOOLS, REVIEW_REPORT_TOOL)),
                    "candidate_path": str(workspace / TOOL_PATH), "candidate_digest": digests[0],
                    "readback": True, "readonly": True, "report": dict(REVIEW_REPORT),
                    "host_evidence": review_evidence(parents[0]["body"]),
                    "parent_request_count": 1, "parent_reply": REVIEW_REPLY,
                    "reviewer_completed": True, "parent_finalized": True,
                    "finalization_evidence": proof}
            ready = case("independent_review", independent_review)
        if ready:
            def resize():
                terminal.resize(72, 24)
                terminal.wait_text(TOOL_REPLY)
                terminal.frame("resize-narrow")
                terminal.resize(120, 36)
                terminal.wait_text(TOOL_REPLY)
                terminal.frame("resize-wide")
                require(TOOL_REPLY in terminal.screen.text(), "rendered reply lost after resize")
            case("resize", resize)
        if terminal:
            case("terminal_restoration", terminal.stop)
            result["metrics"].update(terminal_output_bytes=terminal.bytes, child_exit_code=terminal.exit_code)
        def unchanged():
            require(sha256(binary) == result["binary_sha256"], "binary changed during verification")
            requests, errors = provider.snapshot()
            require(not errors, "fixture protocol errors: " + repr(errors))
            if ready:
                require(len(requests) == 6, f"unexpected additional provider traffic: {len(requests)} requests")
        case("binary_unchanged", unchanged)
    except (BehaviorFailure, AssertionError) as error:
        result["cases"].append({"name": "setup", "status": "failed", "error": str(error)})
    except Exception as error:
        result["cases"].append({"name": "setup", "status": "infrastructure_failed", "error": str(error)})
        if output.is_dir():
            (output / "infrastructure-traceback.txt").write_text(traceback.format_exc())
    finally:
        if terminal and not terminal.closed:
            case("terminal_restoration", terminal.stop)
        if terminal:
            result["metrics"].update(terminal_output_bytes=terminal.bytes,
                                     child_exit_code=terminal.exit_code)
        if provider:
            provider.close()
        completed_names = {entry["name"] for entry in result["cases"]}
        for name in CASE_NAMES:
            if name not in completed_names:
                result["cases"].append({"name": name, "status": "not_run",
                                        "error": "required earlier case did not pass"})
        statuses = [entry["status"] for entry in result["cases"]]
        result["status"] = "infrastructure_failed" if "infrastructure_failed" in statuses else (
            "failed" if any(status != "passed" for status in statuses) else "passed")
        result["completed_at"] = timestamp()
        result["metrics"]["total_ms"] = round((time.monotonic() - started) * 1000, 3)
        rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
        result["metrics"]["child_peak_rss_kib"] = round(rss / 1024 if sys.platform == "darwin" else rss, 3)
        if output.is_dir():
            evidence = [path for path in output.rglob("*") if path.is_file()
                        and path.name not in ("result.json", "result.json.pending")]
            if result["artifacts"].get("tool_output"):
                evidence.append(Path(result["artifacts"]["tool_output"]))
            result["artifact_sha256"] = {str(path): sha256(path) for path in sorted(evidence)}
            temporary = output / "result.json.pending"
            temporary.write_text(json.dumps(result, indent=2) + "\n")
            temporary.replace(output / "result.json")
        print(json.dumps({"status": result["status"], "receipt": str(output / "result.json")}), flush=True)
        signal.signal(signal.SIGTERM, previous_sigterm)
    return {"passed": 0, "failed": 1, "infrastructure_failed": 2}[result["status"]]


if __name__ == "__main__":
    sys.exit(main())
