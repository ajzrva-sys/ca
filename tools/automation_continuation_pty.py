#!/usr/bin/env python3
"""Actual TUI continuation and ordinary automation checks with a fake provider.

All requests, hooks, files, Git worktrees and sessions belong to one disposable
runtime. Interrupted continuations must reuse the exact payload and preserve an
already completed file write. Ordinary automation must execute a real scoped
command under the default required sandbox.
"""
import argparse
import datetime
import hashlib
import http.server
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading

from automation_runtime_pty import Terminal, definition, install_definition, receipts


class Provider(http.server.BaseHTTPRequestHandler):
    requests = []
    effects = {}
    lock = threading.Lock()
    root = None
    ordinary_outputs = []

    def log_message(self, *_):
        pass

    def do_GET(self):
        body = json.dumps({"data": [{"id": "fixture-model"}]}).encode()
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        assert 0 < length <= 8 * 1024 * 1024
        raw = self.rfile.read(length)
        request = json.loads(raw)
        messages = request["messages"]
        user_index = max(index for index, message in enumerate(messages)
                         if message["role"] == "user")
        prompt = messages[user_index]["content"]
        mode = next((name for name in ("AUTO", "MANUAL", "ORDINARY")
                     if "AUT259_PTY_" + name in str(prompt)), None)
        assert mode is not None, prompt
        tool_messages = [message for message in messages[user_index + 1:]
                         if message["role"] == "tool"]
        with Provider.lock:
            assert len(Provider.requests) < 16
            previous = [entry for entry in Provider.requests if entry["mode"] == mode]
            Provider.requests.append({"mode": mode, "raw_sha256": hashlib.sha256(raw).hexdigest(),
                                      "tools": tool_messages, "payload": raw.decode()})
            continuation = len(previous)
        calls = []
        text = ""
        interrupted = False
        if not tool_messages:
            calls = [("write-" + mode, "write_file",
                      {"path": mode.lower() + ".txt", "content": "done"})]
            if mode == "ORDINARY":
                calls.append(("command-ordinary", "run_command",
                              {"command": "printf AUT259_NATIVE_COMMAND_OK", "background": False}))
        elif mode == "ORDINARY":
            Provider.ordinary_outputs = tool_messages
            text = "AUT259_ORDINARY_COMPLETED"
        else:
            path = Provider.root / "repository" / (mode.lower() + ".txt")
            metadata = path.stat()
            identity = [metadata.st_ino, metadata.st_mtime_ns, path.read_text()]
            with Provider.lock:
                if mode in Provider.effects:
                    assert Provider.effects[mode] == identity, "completed write was repeated"
                else:
                    Provider.effects[mode] = identity
            interrupted = continuation <= (2 if mode == "MANUAL" else 1)
            text = "AUT259_" + mode + "_RECOVERED"
        if interrupted:
            body = b'data: {"choices":[{"delta":{},"finish_reason":null}]}\n\n'
        else:
            delta = {"role": "assistant"}
            if calls:
                delta["tool_calls"] = [
                    {"index": index, "id": call_id, "type": "function",
                     "function": {"name": name, "arguments": json.dumps(arguments)}}
                    for index, (call_id, name, arguments) in enumerate(calls)]
            else:
                delta["content"] = text
            chunks = [{"choices": [{"index": 0, "delta": delta, "finish_reason": None}]},
                      {"choices": [{"index": 0, "delta": {},
                                    "finish_reason": "tool_calls" if calls else "stop"}],
                       "usage": {"prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 120}}]
            body = b"".join(("data: " + json.dumps(chunk) + "\n\n").encode() for chunk in chunks)
            body += b"data: [DONE]\n\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(body) + (9999 if interrupted else 0)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)
        self.wfile.flush()
        self.close_connection = True
        if interrupted:
            self.connection.shutdown(socket.SHUT_WR)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix="ca-continuation-pty-", dir="/private/tmp")).resolve()
    os.chmod(root, 0o700)
    Provider.root = root
    repository = root / "repository"
    repository.mkdir()
    (repository / "README.md").write_text("Disposable automation and continuation fixture.\n")
    (repository / ".gitignore").write_text("auto.txt\nmanual.txt\n")
    environment = {"HOME": str(root), "PATH": os.environ.get("PATH", "/opt/homebrew/bin:/usr/bin:/bin"),
                   "TERM": "xterm-256color", "LANG": "en_US.UTF-8", "COLORFGBG": "15;0"}
    for command in (["git", "init", "-q"], ["git", "add", "."],
                    ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@invalid",
                     "commit", "-qm", "fixture"]):
        subprocess.run(command, cwd=repository, env=environment, check=True, capture_output=True)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    config = root / ".config/ca"
    config.mkdir(parents=True, mode=0o700)
    (config / "config.toml").write_text("trusted_workspaces = [" + json.dumps(str(repository)) + "]\n"
        "[[hooks]]\nevent='subagent_start'\ncommand='/usr/bin/true'\nenabled=true\nfail_closed=true\n"
        "[providers.fixture]\nbase_url='http://127.0.0.1:" + str(server.server_port)
        + "/v1'\napi_key=''\nmodels=['fixture-model']\n"
        "[providers.fixture.timeout]\nmax_attempts=1\n"
        "[defaults]\nprovider='fixture'\nmodel='fixture-model'\n"
        "[harness_improvement]\nenabled=false\n")
    os.chmod(config / "config.toml", 0o600)
    value = definition("pty-ordinary", repository, {"kind": "prompt", "prompt": "AUT259_PTY_ORDINARY"}, True)
    value["pricing"] = {"kind": "local_zero"}
    value["schedule"].update(provider="fixture", model="fixture-model", workspace_policy="isolated_pinned",
                              pinned_revision=subprocess.check_output(["git", "rev-parse", "HEAD"],
                                  cwd=repository, env=environment, text=True).strip(),
                              tools=["write_file", "run_command"], grants=[
                                  {"kind": "tool", "tool": "write_file"},
                                  {"kind": "command_prefix", "prefix": ["printf"]}])
    install_definition(binary, value, root, repository, environment)
    state = root / ".local/share/ca/automation-engine/v2"
    result = {"schema": "ca.continuation-pty-receipt.v1", "status": "running", "root": str(root),
              "binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    terminal = Terminal(binary, root, repository, environment)
    try:
        terminal.wait_text("fixture-model", 20)
        terminal.command("AUT259_PTY_AUTO")
        terminal.wait_text("AUT259_AUTO_RECOVERED", 30)
        terminal.command("AUT259_PTY_MANUAL")
        terminal.wait_condition(lambda: len([r for r in Provider.requests if r["mode"] == "MANUAL"]) == 3,
                                "manual recovery did not consume one automatic retry", 30)
        # The scheduled continuation has now failed twice; wait for the actual
        # error state before issuing the user's explicit continuation retry.
        terminal.wait_text("/retry", 10)
        terminal.command("/retry")
        terminal.wait_text("AUT259_MANUAL_RECOVERED", 30)
        for mode, count in (("AUTO", 3), ("MANUAL", 4)):
            requests = [entry for entry in Provider.requests if entry["mode"] == mode]
            assert len(requests) == count, requests
            assert len({entry["raw_sha256"] for entry in requests[1:]}) == 1, requests
            result[mode.lower()] = {"requests": count, "continuation_sha256": requests[1]["raw_sha256"],
                                    "write_identity": Provider.effects[mode]}
        previous = {receipt["run_id"] for receipt in receipts(state)}
        terminal.command("/automation run pty-ordinary")
        terminal.wait_condition(lambda: any(receipt["run_id"] not in previous and receipt["state"]
            in {"succeeded", "failed", "cancelled", "needs_attention"} for receipt in receipts(state)),
            "ordinary automation did not finish", 60)
        completed = next(receipt for receipt in receipts(state) if receipt["run_id"] not in previous)
        assert completed["state"] == "succeeded", completed
        outputs = json.dumps(Provider.ordinary_outputs)
        assert "AUT259_NATIVE_COMMAND_OK" in outputs and "Error running command" not in outputs, outputs
        assert len([entry for entry in Provider.requests if entry["mode"] == "ORDINARY"]) == 2
        result["ordinary"] = {"run_id": completed["run_id"], "state": completed["state"],
                              "command_output": Provider.ordinary_outputs}
        result["status"] = "passed"
    except Exception as error:
        result["status"], result["error"] = "failed", repr(error)
        result["terminal"] = terminal.screen.text()
        raise
    finally:
        terminal.close()
        server.shutdown()
        server.server_close()
        result["terminal_exit"] = terminal.process.returncode
        result["terminal_restored"] = b"\x1b[?1049l" in terminal.raw_tail
        result["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        result["requests"] = Provider.requests
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(json.dumps(result, indent=2) + "\n")
        os.chmod(args.receipt, 0o600)
        print(json.dumps({k: result[k] for k in ("status", "root", "terminal_restored")}), flush=True)
    assert result["terminal_restored"], "alternate screen was not restored"


if __name__ == "__main__":
    main()
