#!/usr/bin/env python3
"""Exercise actual /automation runs while typing in the terminal.

Uses a disposable Git repository, runtime, loopback typed workflow adapter and
offline verification crate. It makes no provider request or live configuration
change. Both workers must finish successfully; responsive failure is not a pass.
"""
import argparse
import datetime
import glob
import hashlib
import http.server
import json
import os
from pathlib import Path
import pty
import fcntl
import signal
import struct
import subprocess
import tempfile
import termios
import threading
import time
import uuid

from bug257_pty import Fixture, Screen


class Terminal(Fixture):
    def __init__(self, binary, root, repository, environment):
        self.screen = Screen(120, 40)
        self.bytes, self.raw_tail = 0, b""
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))

        def terminal_session():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        self.process = subprocess.Popen(
            [str(binary), "--exclude-workspace-context"],
            cwd=repository, stdin=slave, stdout=slave, stderr=slave,
            env=environment, preexec_fn=terminal_session,
        )
        os.close(slave)

    def close_result(self):
        if "q/Esc close" in self.screen.text():
            self.write(b"\x1b")
            self.wait_text("fixture-model", 5)

    def command(self, text):
        self.close_result()
        self.write(b"\x7f" * 200)
        for _ in range(3):
            self.read(0.05)
        self.write(text.encode() + b"\r")

    def close(self):
        if self.process.poll() is None:
            self.close_result()
            self.write(b"\x7f" * 200)
            for _ in range(3):
                self.read(0.05)
        super().close()

    def wait_condition(self, predicate, description, timeout=60):
        deadline = time.monotonic() + timeout
        while not predicate():
            assert self.read(), f"terminal exited: {self.screen.text()}"
            assert time.monotonic() < deadline, f"{description}: {self.screen.text()}"


class Adapter(http.server.BaseHTTPRequestHandler):
    delay = 0
    started = threading.Event()
    requests = 0
    failures = []

    def log_message(self, *_):
        pass

    def do_POST(self):
        try:
            count = int(self.headers.get("Content-Length", "0"))
            assert 0 < count < 65536
            request = json.loads(self.rfile.read(count))
            assert request.get("method") == "session-get", request
            Adapter.requests += 1
            if Adapter.delay:
                Adapter.started.set()
                time.sleep(Adapter.delay)
            body = json.dumps({"result": "success", "arguments": {
                "version": "disposable-fixture", "rpc-version": 14,
            }}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except Exception as error:
            Adapter.failures.append(repr(error))
            self.send_error(500)


def cli(binary, args, repository, environment, request=None):
    result = subprocess.run([str(binary), *args], input=None if request is None else json.dumps(request),
                            text=True, capture_output=True, cwd=repository, env=environment, timeout=60)
    assert result.returncode == 0, f"{' '.join(args)} failed: {result.stdout}\n{result.stderr}"
    return json.loads(result.stdout)


def definition(identity, repository, target, network):
    return {"schema": "ca.automation.v1", "id": identity, "name": identity,
            "automation_kind": {"kind": "standalone"}, "pricing": {"kind": "model_free"},
            "schedule": {"version": 2, "id": identity, "workspace": str(repository),
                         "workspace_policy": "existing_read_only", "allow_network": network,
                         "trigger": {"kind": "interval", "seconds": 86400,
                                     "anchor": "2099-01-01T00:00:00Z"},
                         "target": target, "delivery": {"kind": "none"}}}


def install_definition(binary, value, root, repository, environment):
    path = root / (value["id"] + ".json")
    path.write_text(json.dumps(value))
    preview = cli(binary, ["automation", "--json", "preview", str(path)], repository, environment)
    cli(binary, ["automation", "--json", "create", str(path), "--confirm", preview["confirmation_sha256"]],
        repository, environment)


def receipts(state):
    result = []
    for path in sorted((state / "runs").glob("*/receipt.json")):
        result.append(json.loads(path.read_text()))
    return result


def exercise(terminal, identity, started, state, timeout=60):
    previous = {receipt["run_id"] for receipt in receipts(state)}
    terminal.command("/automation run " + identity)
    def started_or_failed():
        if started():
            return True
        for receipt in receipts(state):
            if receipt["run_id"] not in previous and receipt["state"] not in {"prepared", "running"}:
                raise AssertionError(f"worker finished before its delayed action: {receipt}")
        return False
    terminal.wait_condition(started_or_failed, "owned worker did not reach its delayed action", timeout)
    live = [receipt for receipt in receipts(state) if receipt["run_id"] not in previous]
    assert len(live) == 1 and live[0]["state"] == "running", live
    run = live[0]["run_id"]
    latency = terminal.latency(32)
    assert any(receipt["run_id"] == run and receipt["state"] == "running" for receipt in receipts(state)), \
        "delayed worker finished before input responsiveness was measured"
    terminal.wait_condition(lambda: any(receipt["run_id"] == run and receipt["state"]
                                        in {"succeeded", "failed", "cancelled", "needs_attention"}
                                        for receipt in receipts(state)), "native run did not finish", timeout)
    completed = next(receipt for receipt in receipts(state) if receipt["run_id"] == run)
    assert completed["state"] == "succeeded", completed
    terminal.wait_text("Automation " + identity + " finished", 10)
    terminal.write(b"\x7f" * 200)
    return {"run_id": run, "keyboard_latency": latency, "state": "succeeded"}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--engine", type=Path)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    engine = (args.engine or binary.with_name("codex-team")).resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix="ca-automation-pty-", dir="/private/tmp")).resolve()
    os.chmod(root, 0o700)
    repository = root / "repository"
    repository.mkdir()
    config = root / ".config/ca"
    config.mkdir(parents=True, mode=0o700)
    state = root / ".local/share/ca/automation-engine/v2"
    state.mkdir(parents=True, mode=0o700)
    environment = {"HOME": str(root), "PATH": os.environ.get("PATH", "/opt/homebrew/bin:/usr/bin:/bin"),
                   "TERM": "xterm-256color", "LANG": "en_US.UTF-8", "COLORFGBG": "15;0"}
    (config / "config.toml").write_text("trusted_workspaces = [" + json.dumps(str(repository)) + "]\n"
        "[providers.fixture]\nbase_url='http://127.0.0.1:1/v1'\napi_key=''\nmodels=['fixture-model']\n"
        "[defaults]\nprovider='fixture'\nmodel='fixture-model'\n"
        "[harness_improvement]\nenabled=false\n")
    os.chmod(config / "config.toml", 0o600)
    marker = "pty-" + uuid.uuid4().hex + ".started"
    (repository / "src").mkdir()
    (repository / "Cargo.toml").write_text("[package]\nname='pty_verification_fixture'\nversion='0.1.0'\nedition='2021'\n")
    (repository / "src/lib.rs").write_text("#[test] fn owned_verification() { std::fs::write(" + json.dumps(marker)
        + ",b\"started\").unwrap(); std::thread::sleep(std::time::Duration::from_secs(5)); assert_eq!(2+2,4); }\n")
    (repository / "Cargo.lock").write_text("version = 4\n[[package]]\nname = 'pty_verification_fixture'\nversion = '0.1.0'\n")
    (repository / ".gitignore").write_text("/target/\n/.orchestrator/\n/" + marker + "\n")
    for command in (["git", "init", "-q"], ["git", "add", "."],
                    ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@invalid", "commit", "-qm", "fixture"]):
        subprocess.run(command, cwd=repository, env=environment, check=True, capture_output=True)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Adapter)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    remote = config / "automation-remote.toml"
    remote.write_text("schema='codex-team.remote_config.v1'\n[transmission.fixture]\nurl='http://127.0.0.1:"
                      + str(server.server_port) + "/transmission/rpc'\nprotocol='legacy'\ntimeout_seconds=20\n")
    os.chmod(remote, 0o600)
    draft = {"schema": "codex-team.workflow_draft.v1", "workflow_id": "pty_workflow", "title": "Disposable typed query",
             "actions": [{"id": "query", "depends_on": [], "max_retries": 0,
                          "action": {"kind": "query_transmission", "endpoint": "fixture", "operation": "session"}}],
             "limits": {"max_items": 100, "max_transfer_bytes": 1048576, "max_runtime_seconds": 60,
                        "min_free_space_bytes": 0, "deletion_allowed": False, "conflict_policy": "keep_both",
                        "verification": ["inventory"]},
             "rollback": {"kind": "resume_only", "limitation": "Read-only disposable query"}}
    base = ["workflow", "execute", "--config", str(remote), "--state-root", str(state), "--json"]
    preview = cli(engine, base, repository, environment,
                  {"schema": "codex-team.workflow_request.v1", "operation": {"kind": "preview", "draft": draft, "depth": "mutation_preview"}})["receipt"]
    seed = cli(engine, base, repository, environment,
               {"schema": "codex-team.workflow_request.v1", "operation": {"kind": "run", "workflow_id": "pty_workflow", "draft": draft,
                "accepted_plan_digest": preview["run_plan_digest"]}})
    assert seed["receipt"]["status"] == "completed", seed
    install_definition(binary, definition("pty-workflow", repository, {"kind": "automation_workflow", "workflow_id": "pty_workflow",
        "workflow_digest": preview["workflow_digest"], "resource_binding_digest": preview["resource_binding_digest"]}, True), root, repository, environment)
    install_definition(binary, definition("pty-verification", repository, {"kind": "mechanistic", "actions": [
        {"kind": "git_status_summary", "max_entries": 20},
        {"kind": "evidence", "recipe": {"kind": "cargo_test", "filter": None}, "timeout_seconds": 45}]}, False), root, repository, environment)
    result = {"schema": "ca.automation-pty-receipt.v1", "status": "running", "root": str(root),
              "binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "engine": str(engine), "engine_sha256": hashlib.sha256(engine.read_bytes()).hexdigest(),
              "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(), "provider_requests": 0}
    terminal = Terminal(binary, root, repository, environment)
    try:
        terminal.wait_text("fixture-model", 20)
        Adapter.started.clear()
        Adapter.delay = 5
        result["workflow"] = exercise(terminal, "pty-workflow", Adapter.started.is_set, state)
        Adapter.delay = 0
        scratch = Path("/private/tmp")
        result["verification"] = exercise(terminal, "pty-verification", lambda: bool(glob.glob(
            str(state / "runs" / "*" / "verification-scratch" / "codex-team-verification-*" / "workspace" / marker))
            or glob.glob(str(scratch / "codex-team-verification-*" / "workspace" / marker))), state)
        assert not Adapter.failures, Adapter.failures
        result["adapter_requests"] = Adapter.requests
        result["status"] = "passed"
    except Exception as error:
        result["status"], result["error"] = "failed", repr(error)
        result["terminal"] = terminal.screen.text()
        raise
    finally:
        terminal.close()
        result["terminal_exit"] = terminal.process.returncode
        result["terminal_restored"] = b"\x1b[?1049l" in terminal.raw_tail
        result["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        server.shutdown()
        server.server_close()
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(json.dumps(result, indent=2) + "\n")
        os.chmod(args.receipt, 0o600)
        print(json.dumps(result), flush=True)
    assert result["terminal_restored"], "alternate screen was not restored"


if __name__ == "__main__":
    main()
