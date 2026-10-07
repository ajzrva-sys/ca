#!/usr/bin/env python3
"""Prove legacy observation loss retains the ca-owned occurrence and leases.

Disposable loopback typed adapter only; no provider, live profile or repository
changes. Run against matching built ca/codex-team binaries.
"""
import argparse
import fcntl
import hashlib
import http.server
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import threading
import time


class Adapter(http.server.BaseHTTPRequestHandler):
    delay = 0
    started = threading.Event()
    requests = 0
    errors = []

    def log_message(self, *_):
        pass

    def do_POST(self):
        try:
            size = int(self.headers.get("Content-Length", "0"))
            assert 0 < size < 65536
            request = json.loads(self.rfile.read(size))
            assert request["method"] == "session-get", request
            Adapter.requests += 1
            if Adapter.delay:
                Adapter.started.set()
                time.sleep(Adapter.delay)
            body = json.dumps({"result": "success", "arguments": {
                "version": "compatibility-crash-fixture", "rpc-version": 14,
            }}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except Exception as error:
            Adapter.errors.append(repr(error))
            self.send_error(500)


def invoke(binary, arguments, environment, request):
    output = subprocess.run([str(binary), *arguments], input=json.dumps(request),
                            capture_output=True, text=True, env=environment, timeout=45)
    assert output.returncode == 0, (output.returncode, output.stdout, output.stderr)
    return json.loads(output.stdout)


def wait(predicate, message, seconds=30):
    deadline = time.monotonic() + seconds
    while not predicate():
        assert time.monotonic() < deadline, message
        time.sleep(0.02)


def busy(path):
    with path.open("r+b") as file:
        try:
            fcntl.flock(file, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return True
        return False


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--ca", required=True, type=Path)
    parser.add_argument("--legacy", required=True, type=Path)
    parser.add_argument("--receipt", required=True, type=Path)
    args = parser.parse_args()
    host, legacy = args.ca.resolve(strict=True), args.legacy.resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix="ca-compatibility-crash-")).resolve()
    environment = dict(os.environ, HOME=str(root), CA_AUTOMATION_HOST=str(host))
    environment.pop("CA_AUTOMATION_RESOURCE_FDS", None)
    environment.pop("CA_AUTOMATION_EXECUTOR_FD", None)
    state = root / "legacy-state"
    remote = root / "remote.toml"
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Adapter)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    remote.write_text("schema='codex-team.remote_config.v1'\n[transmission.fixture]\n"
                      f"url='http://127.0.0.1:{server.server_port}/rpc'\nprotocol='legacy'\ntimeout_seconds=20\n")
    remote.chmod(0o600)
    draft = {"schema": "codex-team.workflow_draft.v1", "workflow_id": "crash_fixture",
             "title": "Disposable compatibility lifetime proof", "actions": [
                 {"id": "query", "depends_on": [], "max_retries": 0,
                  "action": {"kind": "query_transmission", "endpoint": "fixture", "operation": "session"}}],
             "limits": {"max_items": 100, "max_transfer_bytes": 1048576, "max_runtime_seconds": 30,
                        "min_free_space_bytes": 0, "deletion_allowed": False,
                        "conflict_policy": "keep_both", "verification": ["inventory"]},
             "rollback": {"kind": "resume_only", "limitation": "Read-only disposable query"}}
    base = ["workflow", "execute", "--config", str(remote), "--state-root", str(state), "--json"]
    preview = invoke(legacy, base, environment, {
        "schema": "codex-team.workflow_request.v1", "operation": {
            "kind": "preview", "draft": draft, "depth": "mutation_preview"}})["receipt"]
    request = {"schema": "codex-team.workflow_request.v1", "operation": {
        "kind": "run", "workflow_id": "crash_fixture", "draft": draft,
        "accepted_plan_digest": preview["run_plan_digest"]}}
    run_id = "compatibility_parent_crash"
    run_root = state / "runs" / run_id
    Adapter.delay = 2
    caller = subprocess.Popen([str(legacy), *base, "--run-id", run_id],
                              stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              env=environment, text=True)
    caller.stdin.write(json.dumps(request))
    caller.stdin.close()
    try:
        def started():
            if caller.poll() is not None:
                raise AssertionError({"root": str(root), "caller_exit": caller.returncode,
                                      "stdout": caller.stdout.read(), "stderr": caller.stderr.read(),
                                      "adapter_requests": Adapter.requests, "adapter_errors": Adapter.errors})
            return Adapter.started.is_set()

        wait(started, "admitted worker did not start its real loopback adapter")
        wait(lambda: (run_root / "context.json").exists(), "durable context was not created")
        context = json.loads((run_root / "context.json").read_text())
        lease = Path(context["lease_root"]) / (hashlib.sha256(b"typed-workflow-mutation").hexdigest() + ".lock")
        assert busy(lease), "worker started without the cross-profile resource claim"
        caller.send_signal(signal.SIGKILL)
        caller.wait(timeout=5)
        caller.stdout.close()
        caller.stderr.close()
        # The observation process is gone while the owned effect is still active.
        time.sleep(0.25)
        assert busy(lease), "caller crash released a still-running worker's resource"

        def completed():
            try:
                return json.loads((run_root / "receipt.json").read_text())["state"] == "succeeded"
            except (FileNotFoundError, json.JSONDecodeError):
                return False

        wait(completed, "ca-owned occurrence failed to complete after observation loss")
        wait(lambda: not busy(lease), "completed worker did not release its resource", 10)
        receipt = json.loads((run_root / "receipt.json").read_text())
        count = Adapter.requests
        replay = invoke(legacy, [*base, "--run-id", run_id], environment, request)
        assert replay == receipt["result"], "duplicate dispatch changed the original V1 result"
        assert Adapter.requests == count, "duplicate dispatch repeated the confirmed effect"
        assert not Adapter.errors, Adapter.errors
        result = {"schema": "ca.compatibility-parent-crash-receipt.v1", "status": "passed",
                  "root": str(root), "caller_exit": caller.returncode, "run_id": run_id,
                  "adapter_requests": count, "resource_retained_after_caller_crash": True,
                  "completion_reconciled_without_effect_repetition": True,
                  "ca": str(host), "legacy": str(legacy)}
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps(result))
    finally:
        if caller.poll() is None:
            caller.kill()
            caller.wait(timeout=5)
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
