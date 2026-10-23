#!/usr/bin/env python3
"""Credentialless native folder export and managed-job progress acceptance.

Uses disposable HOME/XDG/Desktop/workspace directories and one loopback provider.
Retains requests, terminal bytes, visible frames and a receipt. Never installs ca.
"""
import argparse
import datetime
import fcntl
import hashlib
import http.server
import json
import os
from pathlib import Path
import pty
import re
import shlex
import signal
import struct
import subprocess
import tempfile
import termios
import threading
import time
import uuid

from bug257_pty import Screen
from orc258_pty import wait
from plan_workflow_pty import Terminal as BaseTerminal, digest, tool_result


MODEL = "bug292-fixture-model"
PROMPT = "BUG292_FIXTURE: Ask to export to Desktop, write and copy the export, then run the progress job."
PROOF = "BUG292_NATIVE_EXPORT\n"
FINAL = "BUG292_FIXTURE_COMPLETE"


class Provider:
    def __init__(self, desktop):
        self.desktop = desktop
        self.requests, self.errors = [], []
        self.lock = threading.Lock()
        owner = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def reply_json(self, body):
                raw = json.dumps(body).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

            def do_GET(self):
                assert self.path == "/v1/models"
                self.reply_json({"object": "list", "data": [{"id": MODEL, "object": "model"}]})

            def do_POST(self):
                try:
                    assert self.path == "/v1/chat/completions", self.path
                    length = int(self.headers.get("Content-Length", "0"))
                    assert 0 < length <= 8 * 1024 * 1024
                    raw = self.rfile.read(length)
                    body = json.loads(raw)
                    assert body.get("model") == MODEL
                    with owner.lock:
                        assert len(owner.requests) < 12, "unexpected provider polling"
                        owner.requests.append({"path": self.path, "body": body,
                                               "sha256": hashlib.sha256(raw).hexdigest()})
                        calls, text = owner.response(body)
                    message = {"role": "assistant", "content": text}
                    delta = {"role": "assistant", "content": text}
                    if calls:
                        message["tool_calls"] = [{"id": call_id, "type": "function", "function": {
                            "name": name, "arguments": json.dumps(arguments)}}
                            for call_id, name, arguments in calls]
                        delta["tool_calls"] = [{"index": index, **call}
                                               for index, call in enumerate(message["tool_calls"])]
                    finish = "tool_calls" if calls else "stop"
                    response = {"id": "chat-bug292", "object": "chat.completion", "model": MODEL,
                                "choices": [{"index": 0, "message": message, "finish_reason": finish}]}
                    if not body.get("stream"):
                        self.reply_json(response)
                        return
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream")
                    self.send_header("Connection", "close")
                    self.end_headers()
                    for event in [{"choices": [{"index": 0, "delta": delta, "finish_reason": None}]},
                                  {"choices": [{"index": 0, "delta": {}, "finish_reason": finish}]}]:
                        self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
                    self.wfile.write(b"data: [DONE]\n\n")
                    self.wfile.flush()
                    self.close_connection = True
                except (BrokenPipeError, ConnectionResetError):
                    pass
                except Exception as error:
                    with owner.lock:
                        owner.errors.append(repr(error))
                    self.send_error(500, str(error)[:200])

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}/v1"

    def response(self, body):
        names = {tool.get("function", {}).get("name") for tool in body.get("tools", [])}
        native = self.desktop / "native-export.txt"
        copied = self.desktop / "copied-export.txt"
        steps = [
            ("grant", "request_write_access", {"path": str(self.desktop), "reason": "Export the fixture report"}),
            ("native-write", "write_file", {"path": str(native), "content": PROOF}),
            ("copy-export", "run_command", {"command": f"/bin/cp {shlex.quote(str(native))} {shlex.quote(str(copied))}; printf 'BUG292_COPY_OK\\n'"}),
            ("start-progress", "run_command", {"command": "/bin/sh progress.sh", "background": True}),
        ]
        for call_id, name, arguments in steps:
            result = tool_result(body, call_id)
            if result is None:
                assert name in names, f"{name} absent from actual Main catalog"
                return [(call_id, name, arguments)], ""
            if call_id == "grant":
                assert "Write access granted for this live session" in str(result), result
            elif call_id == "native-write":
                assert native.read_text() == PROOF, "native export missing"
            elif call_id == "copy-export":
                assert copied.read_text() == PROOF, "sandboxed command export missing"
                assert "BUG292_COPY_OK" in str(result), result
        started = str(tool_result(body, "start-progress"))
        job = re.search(r"Started background job ([0-9a-f-]{36})", started)
        assert job, started
        if tool_result(body, "await-progress") is None:
            assert "await_job" in names
            return [("await-progress", "await_job", {"job_id": job.group(1), "timeout_secs": 12})], ""
        assert "BUG292_JOB_COMPLETE" in str(tool_result(body, "await-progress"))
        return [], FINAL

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)


class Terminal(BaseTerminal):
    def __init__(self, binary, root, provider):
        self.binary, self.root, self.provider = binary, root, provider
        self.workspace = root / "workspace"
        self.workspace.mkdir()
        self.runtime = root / "home"
        config = self.runtime / ".config/ca"
        config.mkdir(parents=True)
        (self.runtime / "Desktop").mkdir(exist_ok=True)
        self.config = config / "config.toml"
        self.config.write_text(
            "trusted_workspaces=" + json.dumps([str(self.workspace)]) + "\n"
            "[providers.fixture]\nbase_url=" + json.dumps(provider.url) + "\napi_key=''\nmodels=" + json.dumps([MODEL]) + "\n"
            "[defaults]\nprovider='fixture'\nmodel=" + json.dumps(MODEL) + "\n"
            "[agent]\nsandbox_mode='required'\nproactive_delegation=false\nreview_changes=false\n"
            "[harness_improvement]\nenabled=false\n[learning.dream]\nenabled=false\n[learning.distill]\nenabled=false\n")
        self.config_sha256 = digest(self.config)
        (self.workspace / "progress.sh").write_text(
            "printf 'BUG292_STDOUT_ONE\\n'\nprintf 'BUG292_STDERR_ONE\\n' >&2\n"
            "sleep 2\nprintf 'BUG292_STDOUT_TWO\\n'\nprintf 'BUG292_STDERR_TWO\\n' >&2\n"
            "sleep 4\nprintf 'BUG292_JOB_COMPLETE\\n'\n")
        self.environment = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(self.runtime),
                            "XDG_CONFIG_HOME": str(self.runtime / ".config"),
                            "XDG_DATA_HOME": str(self.runtime / ".local/share"),
                            "XDG_CACHE_HOME": str(self.runtime / ".cache"),
                            "TERM": "xterm-256color", "LANG": "en_US.UTF-8", "COLORFGBG": "15;0"}
        self.screen = Screen(120, 36)
        self.bytes, self.raw_tail, self.raw = 0, b"", bytearray()
        self.master, slave = pty.openpty()
        self.original_termios = termios.tcgetattr(slave)
        self.restored_termios = False
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 120, 0, 0))

        def terminal_session():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        self.session_id = str(uuid.uuid4())
        self.process = subprocess.Popen(
            [str(binary), "--profile", "default", "--yolo", "--exclude-workspace-context",
             "--new-session-id", self.session_id, "--name", "BUG292 native export and progress fixture"],
            cwd=self.workspace, stdin=slave, stdout=slave, stderr=slave, env=self.environment,
            preexec_fn=terminal_session)
        os.close(slave)


def scenario(terminal, provider, output):
    terminal.wait_text(MODEL, 20)
    terminal.command(PROMPT)
    terminal.wait_text("Allow writes to", 15)
    wait(lambda: "No administrator password is needed" in " ".join(terminal.screen.text().replace("│", " ").split()), terminal, 5)
    (output / "folder-prompt.txt").write_text(terminal.screen.text())
    terminal.write(b"y")
    terminal.wait_text("stdout: BUG292_STDOUT_ONE", 20)
    terminal.wait_text("stderr: BUG292_STDERR_ONE", 5)
    (output / "progress-initial.txt").write_text(terminal.screen.text())
    requests_before = len(provider.requests)
    initial_elapsed = re.search(r"running · ([0-9]+)s", terminal.screen.text())
    assert initial_elapsed, "pending preview omitted elapsed time"
    initial_elapsed = int(initial_elapsed.group(1))

    def elapsed_advanced():
        match = re.search(r"running · ([0-9]+)s", terminal.screen.text())
        return match and int(match.group(1)) > initial_elapsed

    wait(elapsed_advanced, terminal, 5)
    (output / "progress-timer.txt").write_text(terminal.screen.text())
    assert len(provider.requests) == requests_before, "display progress made provider calls"
    terminal.wait_text("stdout: BUG292_STDOUT_TWO", 5)
    terminal.wait_text("stderr: BUG292_STDERR_TWO", 5)
    (output / "progress-delayed.txt").write_text(terminal.screen.text())
    terminal.wait_text(FINAL, 12)
    (output / "completed.txt").write_text(terminal.screen.text())
    assert (terminal.runtime / "Desktop/native-export.txt").read_text() == PROOF
    assert (terminal.runtime / "Desktop/copied-export.txt").read_text() == PROOF
    assert digest(terminal.config) == terminal.config_sha256, "grant persisted into configuration"
    for line in terminal.raw.decode("utf-8", "replace").splitlines():
        plain = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", line)
        assert not re.search(r"(?i)(?:^|\s)(?:administrator\s+)?password\s*[:?]", plain), "unexpected authentication prompt"
    assert len(provider.requests) == 6, f"unexpected requests: {len(provider.requests)}"
    return {"folder_prompt_answered": True, "native_export": True, "sandboxed_export": True,
            "stdout_and_stderr_live": True, "elapsed_advanced_without_provider_polling": True,
            "fixture_config_unchanged": True, "administrator_password_prompts": 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    output = args.output.absolute()
    output.mkdir(parents=True, exist_ok=False)
    parent = "/private/var/tmp" if Path("/private/var/tmp").exists() else "/var/tmp"
    root = Path(tempfile.mkdtemp(prefix="ca-bug292-pty-", dir=parent)).resolve()
    (root / "home/Desktop").mkdir(parents=True)
    receipt = {"schema": "ca.bug292-pty.v1", "status": "running", "root": str(root),
               "binary": str(binary), "binary_sha256": digest(binary), "harness_sha256": digest(Path(__file__)),
               "started": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    provider, terminal = Provider(root / "home/Desktop"), None

    def interrupted(signum, _frame):
        raise RuntimeError(f"Fixture interrupted by signal {signum}")

    for signum in (signal.SIGTERM, signal.SIGHUP):
        signal.signal(signum, interrupted)
    try:
        terminal = Terminal(binary, root, provider)
        receipt.update(scenario(terminal, provider, output))
        receipt["status"] = "passed"
    except BaseException as error:
        receipt.update(status="failed", error=repr(error))
    finally:
        if terminal:
            (output / "last-screen.txt").write_text(terminal.screen.text())
            terminal.close()
            (output / "terminal.vt").write_bytes(terminal.raw)
            receipt.update(exit_code=terminal.process.returncode, terminal_restored=terminal.restored_termios)
            if terminal.process.returncode != 0 or not terminal.restored_termios:
                receipt.update(status="failed", terminal_error="unclean exit or terminal restoration")
        provider.close()
        (output / "provider-requests.json").write_text(json.dumps(provider.requests, indent=2) + "\n")
        receipt.update(request_count=len(provider.requests), fixture_errors=provider.errors,
                       binary_unchanged=receipt["binary_sha256"] == digest(binary))
        if provider.errors or not receipt["binary_unchanged"]:
            receipt["status"] = "failed"
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, indent=2))
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
