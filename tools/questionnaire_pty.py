#!/usr/bin/env python3
"""Verify live personality steering in bounded, disposable actual CA terminals.

python3 tools/questionnaire_pty.py --binary target/release/ca --receipt scratch/questionnaire/tui.json
Only a loopback fake provider is contacted. Config, editable Markdown, sessions,
tool outputs, and terminal screens belong to the private fixture root. Captured
provider system messages establish steering provenance; visible completion alone
is not treated as proof that the personality reached a request.
"""
import argparse
import datetime
import hashlib
import http.server
import json
from pathlib import Path
import tempfile
import threading
import time

from compaction_pty import Terminal, binary_digest
from orc258_pty import content, wait


A = "PERSONALITY_TUI_SENTINEL_A"
B = "PERSONALITY_TUI_SENTINEL_B"


class Provider:
    protocol = "chat_completions"

    def __init__(self):
        self.lock = threading.Lock()
        self.routes = {}
        self.after_tools = {}
        self.requests = []
        self.failures = []
        self.gates = []
        provider = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def setup(self):
                super().setup()
                self.connection.settimeout(10)

            def do_GET(self):
                body = json.dumps({"data": [{"id": "fixture-model"}], "object": "list"}).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def do_POST(self):
                record = None
                try:
                    length = int(self.headers.get("Content-Length", "0"))
                    assert 0 < length <= 2 * 1024 * 1024, "fixture request exceeded 2 MiB"
                    raw = self.rfile.read(length)
                    payload = json.loads(raw)
                    assert payload.get("stream") is True, "expected the actual streaming TUI path"
                    messages = payload.get("messages", [])
                    user_index = max(index for index, message in enumerate(messages)
                                     if message.get("role") == "user")
                    user = content(messages[user_index])
                    outputs = [{"id": message.get("tool_call_id"), "content": content(message)}
                               for message in messages[user_index + 1:]
                               if message.get("role") == "tool"]
                    systems = [content(message) for message in messages if message.get("role") == "system"]
                    with provider.lock:
                        assert len(provider.requests) < 64, "fixture request ceiling exceeded"
                        record = {"user": user, "system_messages": systems,
                                  "system_sha256": hashlib.sha256("\n".join(systems).encode()).hexdigest(),
                                  "request_sha256": hashlib.sha256(raw).hexdigest(),
                                  "tools": [tool.get("function", {}).get("name")
                                            for tool in payload.get("tools", [])],
                                  "tool_outputs": outputs, "response_sent": False}
                        provider.requests.append(record)
                        response = (provider.after_tools.get(outputs[-1]["id"]) if outputs else None)
                        response = response or provider.routes.get(user)
                    assert response is not None, "unexpected user request in bounded fixture"
                    if response.get("gate"):
                        assert response["gate"].wait(10), "gated fixture response was not released"
                    calls = response.get("calls", [])
                    delta = {"role": "assistant"}
                    if calls:
                        delta["tool_calls"] = [{"index": index, "id": call_id, "type": "function",
                                                "function": {"name": name, "arguments": json.dumps(arguments)}}
                                               for index, (call_id, name, arguments) in enumerate(calls)]
                    else:
                        delta["content"] = response.get("text", "QUESTIONNAIRE_FIXTURE_COMPLETE")
                    events = [{"choices": [{"index": 0, "delta": delta, "finish_reason": None}]},
                              {"choices": [{"index": 0, "delta": {},
                                            "finish_reason": "tool_calls" if calls else "stop"}],
                               "usage": {"prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 120}}]
                    body = "".join("data: " + json.dumps(event) + "\n\n" for event in events)
                    body = (body + "data: [DONE]\n\n").encode()
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream")
                    self.send_header("Content-Length", str(len(body)))
                    self.send_header("Connection", "close")
                    self.end_headers()
                    self.wfile.write(body)
                    self.wfile.flush()
                    self.close_connection = True
                    with provider.lock:
                        record["response_sent"] = True
                except (BrokenPipeError, ConnectionResetError):
                    if record is not None:
                        with provider.lock:
                            record["cancelled_connection"] = True
                except Exception as error:
                    with provider.lock:
                        provider.failures.append(repr(error))
                    try:
                        self.send_error(500)
                    except (BrokenPipeError, ConnectionResetError):
                        pass

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}/v1"

    def arm(self, prompt, text=None, calls=None, gated=False):
        response = {"text": text or "QUESTIONNAIRE_FIXTURE_COMPLETE", "calls": calls or []}
        if gated:
            response["gate"] = threading.Event()
            self.gates.append(response["gate"])
        with self.lock:
            self.routes[prompt] = response
        return response.get("gate")

    def after(self, call_id, text=None, calls=None):
        with self.lock:
            self.after_tools[call_id] = {"text": text or "QUESTIONNAIRE_TOOL_COMPLETE", "calls": calls or []}

    def snapshot(self, start=0):
        with self.lock:
            return [dict(record) for record in self.requests[start:]]

    def close(self):
        for gate in self.gates:
            gate.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)


def personality_path(terminal):
    terminal.cli("harness", "questionnaire", "init")
    path = terminal.runtime / ".config/ca/personality.md"
    assert path.is_file(), "init did not create the editable personality questionnaire"
    return path


def edit(path, marker):
    path.write_text(f"# My permanent CA personality\n{marker}: Be direct, warm, and explain tradeoffs clearly.\n")
    path.chmod(0o600)


def assert_personality(request, expected, absent):
    actual = "\n".join(request["system_messages"])
    assert expected in actual, f"expected {expected} absent from actual provider system messages"
    assert absent not in actual, f"unexpected {absent} leaked into actual provider system messages"


def reload_case(terminal, provider):
    path = personality_path(terminal)
    template = path.read_text()
    path.write_text(template + "\n<!-- UNANSWERED_TUI_QUESTIONNAIRE_CANARY -->\n")
    provider.arm("QUESTIONNAIRE_NEUTRAL_TURN", text="QUESTIONNAIRE_NEUTRAL_COMPLETE")
    terminal.command("QUESTIONNAIRE_NEUTRAL_TURN", "QUESTIONNAIRE_NEUTRAL_COMPLETE", 15)
    terminal.settle()
    neutral = provider.snapshot()[-1]
    systems = "\n".join(neutral["system_messages"])
    assert "UNANSWERED_TUI_QUESTIONNAIRE_CANARY" not in systems
    assert "User-authored CA personality and behavior" not in systems

    edit(path, A)
    provider.arm("QUESTIONNAIRE_FIRST_TURN", text="QUESTIONNAIRE_FIRST_COMPLETE")
    terminal.command("QUESTIONNAIRE_FIRST_TURN", "QUESTIONNAIRE_FIRST_COMPLETE", 15)
    terminal.settle()
    first = provider.snapshot()[-1]
    assert_personality(first, A, B)

    edit(path, B)
    provider.arm("QUESTIONNAIRE_NEXT_TURN", text="QUESTIONNAIRE_NEXT_COMPLETE")
    terminal.command("QUESTIONNAIRE_NEXT_TURN", "QUESTIONNAIRE_NEXT_COMPLETE", 15)
    terminal.settle()
    next_turn = provider.snapshot()[-1]
    assert_personality(next_turn, B, A)
    assert len(provider.snapshot()) == 3
    return {"unanswered_template_is_inert": True, "first_answer_reaches_actual_system_messages": True,
            "next_turn_reloads_changed_answer": True, "completed_visible_turns": 3,
            "scope": "default profile actual TUI; named and bare routes covered by CLI integration"}


def continuation_case(terminal, provider):
    path = personality_path(terminal)
    edit(path, A)
    prompt, call_id = "QUESTIONNAIRE_TOOL_TURN", "questionnaire-frozen-tool"
    gate = provider.arm(prompt, calls=[(call_id, "run_command", {
        "command": "printf QUESTIONNAIRE_HARMLESS_TOOL_OUTPUT", "background": False})], gated=True)
    provider.after(call_id, text="QUESTIONNAIRE_TOOL_TURN_COMPLETE")
    terminal.command(prompt)
    wait(lambda: len(provider.snapshot()) == 1, terminal, timeout=10)
    assert_personality(provider.snapshot()[0], A, B)
    edit(path, B)
    terminal.settle(0.2)
    gate.set()
    terminal.wait_text("QUESTIONNAIRE_TOOL_TURN_COMPLETE", 15)
    terminal.settle()
    requests = provider.snapshot()
    assert len(requests) == 2, "tool turn should use one initial and one continuation request"
    for request in requests:
        assert_personality(request, A, B)
    assert any(output["id"] == call_id and "QUESTIONNAIRE_HARMLESS_TOOL_OUTPUT" in output["content"]
               for output in requests[1]["tool_outputs"]), "tool continuation did not return real command output"
    provider.arm("QUESTIONNAIRE_AFTER_TOOL", text="QUESTIONNAIRE_AFTER_TOOL_COMPLETE")
    terminal.command("QUESTIONNAIRE_AFTER_TOOL", "QUESTIONNAIRE_AFTER_TOOL_COMPLETE", 15)
    terminal.settle()
    assert_personality(provider.snapshot()[-1], B, A)
    return {"initial_provider_request_captured_before_file_edit": True,
            "tool_continuation_keeps_original_answer": True, "real_tool_output_returned": True,
            "new_turn_after_tool_reloads_answer": True}


def retry_case(terminal, provider):
    path = personality_path(terminal)
    edit(path, A)
    terminal.command("/ask", "Ask mode", 5)
    terminal.resize(80, 32)
    terminal.settle()
    prompt, first, repeated = "QUESTIONNAIRE_RETRY_TURN", "questionnaire-unavailable-first", "questionnaire-unavailable-repeated"
    forbidden = "printf forbidden > questionnaire-must-not-exist.txt"
    provider.arm(prompt, calls=[(first, "run_command", {"command": forbidden, "background": False})])
    provider.after(first, calls=[(repeated, "run_command", {"command": forbidden, "background": False})])
    terminal.command(prompt, "Ask has no tools", 15)
    terminal.settle()
    initial = provider.snapshot()
    assert len(initial) == 2
    assert all(request["tools"] == [] for request in initial)
    for request in initial:
        assert_personality(request, A, B)
    assert not (terminal.workspace / "questionnaire-must-not-exist.txt").exists()
    edit(path, B)
    provider.arm(prompt, text="QUESTIONNAIRE_RETRY_COMPLETE")
    provider.after(first, text="QUESTIONNAIRE_RETRY_COMPLETE")
    provider.after(repeated, text="QUESTIONNAIRE_RETRY_COMPLETE")
    terminal.command("/retry", "QUESTIONNAIRE_RETRY_COMPLETE", 20)
    terminal.settle()
    requests = provider.snapshot()
    assert len(requests) == 3, "explicit retry should send exactly one frozen request"
    assert_personality(requests[-1], A, B)
    assert requests[-1]["tools"] == [], "retry widened the Ask tool catalog"
    assert not (terminal.workspace / "questionnaire-must-not-exist.txt").exists()
    provider.arm("QUESTIONNAIRE_AFTER_RETRY", text="QUESTIONNAIRE_AFTER_RETRY_COMPLETE")
    terminal.command("QUESTIONNAIRE_AFTER_RETRY", "QUESTIONNAIRE_AFTER_RETRY_COMPLETE", 15)
    terminal.settle()
    assert_personality(provider.snapshot()[-1], B, A)
    return {"explicit_retry_keeps_original_answer": True, "retry_keeps_original_empty_tool_catalog": True,
            "unavailable_command_never_executes": True, "next_turn_after_retry_reloads_answer": True}


def responsiveness_case(terminal, provider):
    path = personality_path(terminal)
    edit(path, A)
    gate = provider.arm("QUESTIONNAIRE_ESCAPE_PENDING", text="QUESTIONNAIRE_STALE_RESPONSE", gated=True)
    terminal.command("QUESTIONNAIRE_ESCAPE_PENDING")
    wait(lambda: len(provider.snapshot()) == 1, terminal, timeout=10)
    assert_personality(provider.snapshot()[0], A, B)
    started = time.monotonic()
    terminal.write(b"\x1b")
    terminal.wait_text("Stopped", 3)
    escape_ms = round((time.monotonic() - started) * 1000, 3)
    gate.set()
    terminal.settle(0.3)
    assert "QUESTIONNAIRE_STALE_RESPONSE" not in terminal.screen.text()
    edit(path, B)
    provider.arm("QUESTIONNAIRE_AFTER_ESCAPE", text="QUESTIONNAIRE_AFTER_ESCAPE_COMPLETE")
    terminal.command("QUESTIONNAIRE_AFTER_ESCAPE", "QUESTIONNAIRE_AFTER_ESCAPE_COMPLETE", 15)
    terminal.settle()
    assert_personality(provider.snapshot()[-1], B, A)
    ctrl_gate = provider.arm("QUESTIONNAIRE_CTRL_C_PENDING", text="QUESTIONNAIRE_CTRL_C_STALE", gated=True)
    previous = len(provider.snapshot())
    terminal.command("QUESTIONNAIRE_CTRL_C_PENDING")
    wait(lambda: len(provider.snapshot()) == previous + 1, terminal, timeout=10)
    assert_personality(provider.snapshot()[-1], B, A)
    started = time.monotonic()
    terminal.write(b"\x03")
    deadline = started + 3
    while terminal.process.poll() is None and "Stopped" not in terminal.screen.text():
        terminal.read(0.03)
        assert time.monotonic() < deadline, "Ctrl-C did not stop or exit the pending turn promptly"
    outcome = "exited" if terminal.process.poll() is not None else "stopped"
    ctrl_c_ms = round((time.monotonic() - started) * 1000, 3)
    ctrl_gate.set()
    return {"escape_stops_pending_provider": True, "escape_ms": escape_ms,
            "cancelled_response_does_not_publish": True, "new_turn_after_escape_reloads_answer": True,
            "ctrl_c_outcome": outcome, "ctrl_c_ms": ctrl_c_ms,
            "scope": "pending loopback HTTP after asynchronous behavior preparation; no filesystem latency percentile claim"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--receipt", type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    fixture_parent = "/private/tmp" if Path("/private/tmp").is_dir() else None
    root = Path(tempfile.mkdtemp(prefix="ca-questionnaire-pty-", dir=fixture_parent)).resolve()
    root.chmod(0o700)
    receipt = {"schema": "ca.questionnaire-pty-receipt.v1", "status": "running", "root": str(root),
               "binary": str(binary), "binary_sha256": binary_digest(binary), "cases": {},
               "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    cases = {"live-next-turn-reload": reload_case, "frozen-tool-continuation": continuation_case,
             "frozen-explicit-retry": retry_case, "escape-and-ctrl-c": responsiveness_case}
    for name, case in cases.items():
        case_root = root / name
        case_root.mkdir(mode=0o700)
        provider = Provider()
        terminal = None
        result = {"status": "running"}
        try:
            terminal = Terminal(binary, case_root, provider)
            result["checks"] = case(terminal, provider)
            assert not provider.failures, provider.failures
            result["status"] = "passed"
        except Exception as error:
            result["status"] = "failed"
            result["error"] = str(error)
            result["screen"] = terminal.screen.text() if terminal else "startup failed"
        finally:
            # Release pending fixture I/O before closing the owned terminal.
            for gate in provider.gates:
                gate.set()
            if terminal:
                result["output_bytes"] = terminal.bytes
                terminal.close()
                result["exit_code"] = terminal.process.returncode
                result["terminal_restored"] = terminal.restored_termios
                if terminal.process.returncode or not terminal.restored_termios:
                    result["status"] = "failed"
                    result.setdefault("error", "terminal failed to exit cleanly and restore termios")
            provider.close()
            result["provider_requests"] = provider.snapshot()
            result["provider_failures"] = provider.failures
            if provider.failures:
                result["status"] = "failed"
            receipt["cases"][name] = result
    receipt["status"] = "passed" if all(case["status"] == "passed" for case in receipt["cases"].values()) else "failed"
    receipt["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    output = root / "result.json"
    output.write_text(json.dumps(receipt, indent=2) + "\n")
    if args.receipt:
        target = args.receipt.resolve()
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"status": receipt["status"], "root": str(root), "result": str(output),
                      "binary_sha256": receipt["binary_sha256"],
                      "cases": {name: {key: value for key, value in case.items()
                                       if key not in ("screen", "provider_requests", "provider_failures")}
                                for name, case in receipt["cases"].items()}}), flush=True)
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
