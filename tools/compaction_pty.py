#!/usr/bin/env python3
"""Exercise checkpoint token exhaustion in disposable actual terminal sessions.

python3 tools/compaction_pty.py --binary target/release/ca --receipt /private/tmp/compaction.json
Only a loopback fake provider is contacted. Homes, configuration, workspaces and
sessions are private fixture files. No installed binary or live profile is changed.
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
import struct
import subprocess
import tempfile
import termios
import threading
import time
import uuid

from orc258_pty import Walkthrough, content, wait
from bug257_pty import Screen


INITIAL_CAP = 16_384
RETRY_CAP = 32_768
VISIBLE_SUMMARY_CAP = 4_096
PARTIAL = "COMPACTION_INCOMPLETE_CANDIDATE_MUST_NEVER_PUBLISH"
REASONING = "COMPACTION_PRIVATE_REASONING_MUST_NEVER_PUBLISH"
SECTIONS = ("Task", "Constraints", "Facts", "Hypotheses", "Files Touched", "Decisions", "Test State", "Open Issues")


def summary(marker):
    return "\n".join(f"## {section}\n" + (marker if section == "Facts" else "(none)") + "\n" for section in SECTIONS)


def binary_digest(path):
    digest = hashlib.sha256()
    with path.open("rb") as executable:
        for block in iter(lambda: executable.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


class Provider:
    def __init__(self, protocol):
        self.protocol = protocol
        self.lock = threading.Lock()
        self.requests = []
        self.failures = []
        self.responses = []
        provider = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_GET(self):
                self.send_json({"data": [{"id": "fixture-model", "object": "model"}], "object": "list"})

            def send_json(self, value):
                body = json.dumps(value).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def do_POST(self):
                record = None
                try:
                    length = int(self.headers.get("Content-Length", "0"))
                    assert 0 < length <= 8 * 1024 * 1024
                    raw = self.rfile.read(length)
                    payload = json.loads(raw)
                    messages = payload.get("messages", payload.get("input", []))
                    prompt = next((content(message) for message in reversed(messages) if message.get("role") == "user"), "")
                    checkpoint = prompt.startswith("Create a compact checkpoint for the coding session transcript below.")
                    with provider.lock:
                        assert len(provider.requests) < 128, "fixture request ceiling exceeded"
                        index = len(provider.requests)
                        record = {"checkpoint": checkpoint, "prompt": prompt,
                            "prompt_sha256": hashlib.sha256(prompt.encode()).hexdigest(),
                            "path": self.path, "stream": payload.get("stream"),
                            "cap": payload.get("max_output_tokens", payload.get("max_completion_tokens", payload.get("max_tokens"))),
                            "tool_count": len(payload.get("tools", [])),
                            "request_sha256": hashlib.sha256(raw).hexdigest(), "response_sent": False}
                        provider.requests.append(record)
                        if checkpoint:
                            assert provider.responses, "unexpected checkpoint request"
                            response = provider.responses.pop(0)
                        else:
                            response = {"text": prompt + "_REPLY" if prompt.startswith("COMPACTION_SEED_") else "COMPACTION_FIXTURE_REPLY"}
                    time.sleep(response.get("delay", 0.2 if checkpoint else 0))
                    text = response.get("text", PARTIAL)
                    reasoning = REASONING if checkpoint else "Fixture seed reasoning"
                    mode = response.get("mode", "complete")
                    if mode == "http_error":
                        self.send_error(503)
                        with provider.lock:
                            record["response_sent"] = True
                        return
                    if not payload.get("stream"):
                        self.send_json({"choices": [{"message": {"role": "assistant", "content": text}, "finish_reason": "stop"}]})
                    else:
                        self.send_response(200)
                        self.send_header("Content-Type", "text/event-stream")
                        self.send_header("Connection", "close")
                        self.send_header("x-request-id", f"compaction-fixture-{index}")
                        self.end_headers()
                        if provider.protocol == "chat_completions":
                            events = [{"choices": [{"index": 0, "delta": {"role": "assistant", "content": text,
                                "reasoning_content": reasoning}, "finish_reason": None}]}]
                            finish = "length" if mode == "limit" else "content_filter" if mode == "filter" else "stop"
                            if mode != "eof":
                                events.append({"choices": [{"index": 0, "delta": {}, "finish_reason": finish}],
                                    "usage": {"prompt_tokens": 100, "completion_tokens": 12_500,
                                        "total_tokens": 12_600, "completion_tokens_details": {"reasoning_tokens": 12_000}}})
                        else:
                            events = [{"type": "response.reasoning_summary_text.delta", "output_index": 0, "summary_index": 0, "delta": reasoning},
                                {"type": "response.output_text.delta", "output_index": 1, "content_index": 0, "delta": text}]
                            terminal = {"id": f"compaction_fixture_{index}", "model": "fixture-model", "status": "completed",
                                "usage": {"input_tokens": 100, "output_tokens": 12_500,
                                    "total_tokens": 12_600, "output_tokens_details": {"reasoning_tokens": 12_000}}}
                            if mode in ("limit", "filter", "unknown"):
                                terminal.update({"status": "incomplete", "incomplete_details": {"reason": {
                                    "limit": "max_output_tokens", "filter": "content_filter", "unknown": "future_provider_reason"}[mode]}})
                                events.append({"type": "response.incomplete", "response": terminal})
                            elif mode == "refusal":
                                events.append({"type": "response.refusal.delta", "delta": "Untrusted provider refusal body"})
                            elif mode != "eof":
                                events.append({"type": "response.completed", "response": terminal})
                        try:
                            for event in events:
                                self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
                            if provider.protocol == "chat_completions" and mode != "eof":
                                self.wfile.write(b"data: [DONE]\n\n")
                            self.wfile.flush()
                        except (BrokenPipeError, ConnectionResetError):
                            pass  # Cancellation intentionally closes the pending HTTP stream.
                        self.close_connection = True
                    with provider.lock:
                        record["response_sent"] = True
                except (BrokenPipeError, ConnectionResetError):
                    if record is not None:
                        with provider.lock:
                            record["response_sent"] = True
                except Exception as error:
                    with provider.lock:
                        provider.failures.append(repr(error))
                    self.send_error(500)

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}/v1"

    def arm(self, *responses):
        with self.lock:
            assert not self.responses, "unused checkpoint responses from previous scenario"
            self.responses = list(responses)

    def checkpoint_requests(self, start=0):
        with self.lock:
            return [dict(request) for request in self.requests[start:] if request["checkpoint"]]

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)


class Terminal(Walkthrough):
    """Reuse the existing real-terminal command/save reader with a protocol fixture."""
    def __init__(self, binary, root, provider):
        self.binary, self.root, self.provider = binary, root, provider
        self.workspace = root / "workspace"
        self.workspace.mkdir()
        self.runtime = root / "runtime"
        config = self.runtime / ".config/ca"
        config.mkdir(parents=True)
        configuration = ("trusted_workspaces = [" + json.dumps(str(self.workspace)) + "]\n"
            "[providers.fixture]\nbase_url=" + json.dumps(provider.url) + "\napi_key=''\nmodels=['fixture-model']\n"
            "protocol=" + json.dumps(provider.protocol) + "\n"
            "[defaults]\nprovider='fixture'\nmodel='fixture-model'\n"
            "[harness_improvement]\nenabled=false\n[learning.dream]\nenabled=false\n[learning.distill]\nenabled=false\n")
        (config / "config.toml").write_text(configuration)
        self.environment = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(self.runtime),
            "TERM": "xterm-256color", "LANG": "en_US.UTF-8", "COLORFGBG": "15;0"}
        self.screen = Screen(140, 45)
        self.bytes, self.raw_tail = 0, b""
        self.master, slave = pty.openpty()
        self.original_termios = termios.tcgetattr(slave)
        self.restored_termios = False
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 45, 140, 0, 0))

        def terminal_session():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        self.session_id = str(uuid.uuid4())
        self.process = subprocess.Popen([str(binary), "--profile", "default", "--yolo", "--exclude-workspace-context",
            "--new-session-id", self.session_id, "--name", "Compaction token-limit fixture"], cwd=self.workspace,
            stdin=slave, stdout=slave, stderr=slave, env=self.environment, preexec_fn=terminal_session)
        os.close(slave)
        try:
            self.wait_text("fixture-model", 20)
        except BaseException:
            self.close()
            raise

    def close(self):
        if self.process.poll() is None:
            self.write(b"\x03")
            deadline = time.monotonic() + 5
            while self.process.poll() is None and time.monotonic() < deadline:
                self.read(0.05)
        if self.process.poll() is None:
            self.process.terminate()
        self.process.wait(timeout=5)
        self.restored_termios = termios.tcgetattr(self.master) == self.original_termios
        os.close(self.master)


def compact(terminal, provider, responses, expect, count):
    start = len(provider.requests)
    provider.arm(*responses)
    terminal.command("/compact here 1", "Creating context checkpoint", 5)
    wait(lambda: len(provider.checkpoint_requests(start)) >= count, terminal, timeout=10)
    wait(lambda: all(request["response_sent"] for request in provider.checkpoint_requests(start)), terminal, timeout=10)
    terminal.wait_text(expect, 10)
    terminal.settle(0.3)
    requests = provider.checkpoint_requests(start)
    assert len(requests) == count, f"expected {count} physical requests, got {len(requests)}"
    assert all(request["stream"] and request["cap"] > VISIBLE_SUMMARY_CAP for request in requests), requests
    assert all(request["tool_count"] == 0 for request in requests), "checkpoint request advertised tools"
    assert "4096" in requests[0]["prompt"] and "32 KiB" in requests[0]["prompt"], "source prompt omitted visible-summary bounds"
    assert not provider.failures, provider.failures
    return requests


def checkpoint(terminal):
    path, saved = terminal.save()
    value = saved["context_checkpoint"]
    metadata = json.loads(path.read_text())
    if metadata.get("storage_version"):
        history = path.parent / (metadata["storage_key"] + "." + metadata["generation"] + ".turns.jsonl")
        raw = history.read_bytes()[:metadata["committed_bytes"]]
        assert hashlib.sha256(raw).hexdigest() == metadata["history_sha256"]
    return value, metadata.get("history_sha256")


def usage_counts(terminal):
    terminal.command("/usage", "This process", 5)
    terminal.settle(0.3)
    screen = terminal.screen.text()
    match = re.search(r"This process\s+(\d+) requests · (\d+) in · (\d+) out", screen)
    assert match, "usage modal omitted process accounting: " + screen
    terminal.dismiss()
    return tuple(int(value) for value in match.groups()), screen


def exercise(binary, root, protocol, reproduce=False):
    root.mkdir(mode=0o700)
    provider = Provider(protocol)
    terminal = None
    result = {"status": "running", "checks": {}}
    try:
        terminal = Terminal(binary, root, provider)
        for index in range(3):
            marker = f"COMPACTION_SEED_{index}"
            terminal.command(marker, marker + "_REPLY", 10)
            terminal.settle(0.2)
        terminal.save()
        if reproduce:
            provider.arm({"text": summary("COMPACTION_BEFORE_FIX_BASELINE")})
            terminal.command("/compact here 1", "Context checkpoint created", 10)
            prior = checkpoint(terminal)
            start = len(provider.requests)
            provider.arm({"mode": "limit"})
            terminal.command("/compact here 1", "Creating context checkpoint", 5)
            terminal.wait_text("Compaction failed: provider response was incomplete", 10)
            terminal.settle(0.3)
            requests = provider.checkpoint_requests(start)
            assert len(requests) == 1 and requests[0]["response_sent"], requests
            result["screen"] = terminal.screen.text()
            assert checkpoint(terminal) == prior
            result["checks"] = {"exact_screenshot_footer_reproduced": True, "previous_checkpoint_kept": True,
                "old_request_cap": requests[0]["cap"]}
            result["status"] = "reproduced"
            return result
        requests = compact(terminal, provider, [{"text": summary("COMPACTION_BASELINE")}], "Context checkpoint created", 1)
        assert requests[0]["cap"] == INITIAL_CAP
        original, original_history = checkpoint(terminal)
        assert original["format_version"] == 2 and len(original["source_sha256"]) == 64
        assert original["through_message_count"] == 4
        result["checks"]["reasoning_budget_above_visible_summary_cap"] = True
        before_usage, _ = usage_counts(terminal)
        requests = compact(terminal, provider, [{"mode": "limit"}, {"text": summary("COMPACTION_RETRY_COMPLETE")}], "Context checkpoint created", 2)
        assert [request["cap"] for request in requests] == [INITIAL_CAP, RETRY_CAP]
        assert requests[0]["prompt"] == requests[1]["prompt"], "retry prompt/source changed"
        assert PARTIAL not in requests[1]["prompt"] and REASONING not in requests[1]["prompt"]
        prior, history = checkpoint(terminal)
        assert "COMPACTION_RETRY_COMPLETE" in prior["summary"] and history == original_history
        assert PARTIAL not in prior["summary"] and REASONING not in prior["summary"]
        result["checks"]["one_fresh_retry_saved_valid_checkpoint"] = True
        after_usage, result["retry_usage_screen"] = usage_counts(terminal)
        assert after_usage[0] == before_usage[0] + 2, "physical checkpoint requests missing from usage ledger"
        if protocol == "chat_completions":
            assert after_usage[1:] == (before_usage[1] + 200, before_usage[2] + 25_000), "failed length attempt usage missing"
            result["checks"]["failed_length_attempt_usage_retained"] = True
        else:
            result["usage_note"] = "Responses incomplete usage is not published by the existing decoder; physical attempts are individually recorded."
        requests = compact(terminal, provider, [{"mode": "limit"}, {"mode": "limit"}], "output token limit", 2)
        assert [request["cap"] for request in requests] == [INITIAL_CAP, RETRY_CAP]
        assert requests[0]["prompt"] == requests[1]["prompt"]
        failure_screen = terminal.screen.text()
        assert "Compaction failed:" in failure_screen and "/compact" in failure_screen
        assert checkpoint(terminal) == (prior, history), "repeated truncation replaced checkpoint/history"
        result["checks"]["repeated_limit_truthful_failure_preserves_checkpoint"] = True
        result["repeated_limit_screen"] = failure_screen
        requests = compact(terminal, provider, [{"text": "Invalid completed checkpoint"}, {"mode": "limit"},
            {"text": summary("COMPACTION_QUALITY_AND_LIMIT_COMPLETE")}], "Context checkpoint created", 3)
        assert "only repair attempt" in requests[1]["prompt"] and requests[1]["prompt"] == requests[2]["prompt"]
        assert requests[1]["cap"] == INITIAL_CAP and requests[2]["cap"] == RETRY_CAP
        prior, history = checkpoint(terminal)
        assert "COMPACTION_QUALITY_AND_LIMIT_COMPLETE" in prior["summary"]
        result["checks"]["quality_repair_can_use_one_token_retry"] = True
        requests = compact(terminal, provider, [{"mode": "limit"}, {"text": "Invalid retry checkpoint"},
            {"mode": "limit"}], "output token limit", 3)
        assert requests[0]["prompt"] == requests[1]["prompt"] and "only repair attempt" in requests[2]["prompt"]
        assert checkpoint(terminal) == (prior, history), "shared retry exhaustion replaced checkpoint/history"
        result["checks"]["quality_repair_cannot_reset_token_retry_budget"] = True
        modes = [("filter", "incomplete"), ("eof", "finish reason"), ("http_error", "provider HTTP 503")]
        if protocol == "openai_responses":
            modes = [("filter", "incomplete"), ("refusal", "refused"), ("unknown", "incomplete"),
                ("eof", "finish reason"), ("http_error", "provider HTTP 503")]
        for mode, diagnostic in modes:
            compact(terminal, provider, [{"mode": mode}], diagnostic, 1)
            assert "Compaction failed:" in terminal.screen.text()
            assert "output token limit" not in terminal.screen.text()
            assert checkpoint(terminal) == (prior, history), f"{mode} replaced checkpoint/history"
            result["checks"][mode + "_does_not_retry_or_publish"] = True
        start = len(provider.requests)
        provider.arm({"mode": "limit"}, {"text": summary("COMPACTION_CANCELLED_CANDIDATE"), "delay": 1.5})
        terminal.command("/compact here 1", "Creating context checkpoint", 5)
        wait(lambda: len(provider.checkpoint_requests(start)) == 2, terminal, timeout=10)
        terminal.write(b"\x1b")
        terminal.wait_text("Compaction cancelled", 5)
        terminal.settle(1.8)
        assert checkpoint(terminal) == (prior, history), "cancelled retry published checkpoint/history"
        assert len(provider.checkpoint_requests(start)) == 2
        result["checks"]["escape_cancels_retry_without_publication"] = True
        assert not provider.failures, provider.failures
        result["status"] = "passed"
    except Exception as error:
        result["status"] = "failed"
        result["error"] = str(error)
        result["screen"] = terminal.screen.text() if terminal else "startup failed"
    finally:
        if terminal:
            result["output_bytes"] = terminal.bytes
            terminal.close()
            result["exit_code"] = terminal.process.returncode
            result["terminal_restored"] = terminal.restored_termios
            if terminal.process.returncode or not terminal.restored_termios:
                result["status"] = "failed"
                result.setdefault("error", "terminal did not exit successfully and restore termios")
        provider.close()
        result["provider_request_receipts"] = provider.requests
        result["provider_failures"] = provider.failures
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--protocol", action="append", choices=("chat_completions", "openai_responses"))
    parser.add_argument("--receipt", type=Path)
    parser.add_argument("--reproduce", action="store_true", help="Record the exact old incomplete footer before the fix")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix="ca-compaction-pty-", dir="/private/tmp")).resolve()
    root.chmod(0o700)
    receipt = {"status": "running", "root": str(root), "binary": str(binary),
        "binary_sha256": binary_digest(binary), "cases": {},
        "mode": "reproduce_before_fix" if args.reproduce else "verify_fix",
        "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    for protocol in args.protocol or ("chat_completions", "openai_responses"):
        receipt["cases"][protocol] = exercise(binary, root / protocol, protocol, args.reproduce)
    expected = "reproduced" if args.reproduce else "passed"
    receipt["status"] = expected if all(case["status"] == expected for case in receipt["cases"].values()) else "failed"
    receipt["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    output = root / "result.json"
    output.write_text(json.dumps(receipt, indent=2) + "\n")
    if args.receipt:
        args.receipt.resolve().write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"status": receipt["status"], "root": str(root), "result": str(output),
        "binary_sha256": receipt["binary_sha256"], "cases": {name: {key: (value[:600] if key == "error" else value) for key, value in case.items()
            if key not in ("screen", "repeated_limit_screen", "retry_usage_screen", "provider_request_receipts")} for name, case in receipt["cases"].items()}}), flush=True)
    return 0 if receipt["status"] == expected else 1


if __name__ == "__main__":
    raise SystemExit(main())
