#!/usr/bin/env python3
"""Exercise automatic frozen-request recovery in the actual TUI.

Only a loopback fake provider and disposable persistent sessions are used. The
first stream ends after a valid partial delta; the replacement must save a new
branch and resend the same request once, preserving the original partial answer.
A temporary directory write denial must retain useful errors and send no retry.
"""
import argparse
import datetime
import hashlib
import http.server
import json
import os
from pathlib import Path
import tempfile
import threading
import time

from orc258_pty import Walkthrough, wait


PROMPT = "RECOVERY_BRANCH_FROZEN_PTY_REQUEST"
PARTIAL = "RECOVERY_BRANCH_ORIGINAL_PARTIAL"
REPLY = "RECOVERY_BRANCH_REPLACEMENT_COMPLETE"


class Provider:
    def __init__(self):
        self.requests = []
        self.failures = []
        self.lock = threading.Lock()
        self.before_first_eof = None
        provider = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_GET(self):
                body = json.dumps({"data": [{"id": "fixture-model"}]}).encode()
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def do_POST(self):
                try:
                    length = int(self.headers.get("Content-Length", "0"))
                    assert 0 < length <= 8 * 1024 * 1024
                    raw = self.rfile.read(length)
                    payload = json.loads(raw)
                    assert payload.get("stream") is True, payload
                    users = [message for message in payload["messages"] if message["role"] == "user"]
                    assert users[-1]["content"] == PROMPT, users
                    with provider.lock:
                        attempt = len(provider.requests)
                        provider.requests.append({"raw_sha256": hashlib.sha256(raw).hexdigest(),
                                                  "payload": payload})
                    assert attempt < 2, "more than one replacement was sent"
                    chunk = {"choices": [{"index": 0, "delta": {
                        "role": "assistant", "content": PARTIAL if attempt == 0 else REPLY},
                        "finish_reason": None}]}
                    body = ("data: " + json.dumps(chunk) + "\n\n").encode()
                    if attempt:
                        finish = {"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                                  "usage": {"prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 120}}
                        body += ("data: " + json.dumps(finish) + "\n\n").encode()
                        body += b"data: [DONE]\n\n"
                    # The first HTTP response is valid but its SSE stream has
                    # neither a finish reason nor DONE: a real premature EOF.
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream")
                    self.send_header("Content-Length", str(len(body)))
                    self.send_header("Connection", "close")
                    self.end_headers()
                    if attempt == 0 and provider.before_first_eof:
                        provider.before_first_eof()
                    self.wfile.write(body)
                    self.wfile.flush()
                    self.close_connection = True
                except Exception as error:
                    provider.failures.append(repr(error))
                    self.send_error(500)

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}/v1"

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)


def sessions(terminal):
    result = []
    directory = terminal.runtime / ".local/share/ca/sessions"
    for path in sorted(directory.glob("*.json")):
        metadata = json.loads(path.read_text())
        record = metadata.get("session", metadata)
        messages = record.get("messages", [])
        if metadata.get("storage_version"):
            history = directory / (metadata["storage_key"] + "." + metadata["generation"] + ".turns.jsonl")
            raw = history.read_bytes()[:metadata["committed_bytes"]]
            assert hashlib.sha256(raw).hexdigest() == metadata["history_sha256"]
            messages = [json.loads(line)["message"] for line in raw.splitlines()]
            assert len(messages) == metadata["message_count"]
        result.append({"id": record["id"], "title": record["title"], "parent": record.get("parent"),
                       "metadata_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                       "messages": messages, "path": str(path)})
    return result


def exercise(binary, root, width, height, save_failure=False):
    root.mkdir(mode=0o700)
    provider = Provider()
    terminal = None
    case = {"width": width, "height": height, "save_failure": save_failure, "status": "running"}
    directory = None
    previous_mode = None
    try:
        terminal = Walkthrough(binary, root, provider, name="Automatic recovery fixture")
        case["source_id"] = terminal.session_id
        if save_failure:
            directory = terminal.runtime / ".local/share/ca/sessions"
            directory.mkdir(parents=True, exist_ok=True)
            previous_mode = directory.stat().st_mode & 0o777

            def deny_source_save():
                directory.chmod(0o500)
                # Prove this is a real filesystem write failure, rather than a
                # fake provider error or an injected application event.
                try:
                    with (directory / "fixture-write-probe").open("xb"):
                        pass
                except PermissionError:
                    case["real_write_denial_confirmed"] = True
                else:
                    (directory / "fixture-write-probe").unlink()
                    raise AssertionError("temporary directory did not deny writes")

            provider.before_first_eof = deny_source_save
        terminal.resize(width, height)
        terminal.settle()
        started = time.monotonic()
        terminal.command(PROMPT)
        terminal.wait_text(PARTIAL, 5)
        case["partial_screen"] = terminal.screen.text()
        wait(lambda: REPLY in terminal.screen.text() or "no replacement sent" in terminal.screen.text().lower(),
             terminal, timeout=15)
        case["outcome_screen"] = terminal.screen.text()
        if "no replacement sent" in terminal.screen.text().lower():
            case["save_failure_footer_observed"] = True
            assert len(provider.requests) == 1, "replacement was sent after branch save failure"
            if not save_failure:
                raise AssertionError("automatic recovery branch save failed; no replacement sent")
            assert case.get("real_write_denial_confirmed"), "save failure did not use real write denial"
            terminal.command("/errors")
            terminal.wait_text("Recent provider failures", 5)

            def useful_error():
                text = " ".join(terminal.screen.text().split()).lower()
                return any(marker in text for marker in (
                    "permission denied", "operation not permitted", "os error 13"))

            wait(useful_error, terminal, timeout=5)
            case["useful_error_observed"] = True
            case["error_detail_screen"] = terminal.screen.text()
            # Observe beyond the normal one-second automatic retry delay.
            terminal.settle(1.3)
            assert len(provider.requests) == 1, "replacement was sent after genuine save failure"
            assert not any(value["parent"] for value in sessions(terminal)), "failure created a branch"
        else:
            assert not save_failure, "replacement was sent despite real persistence failure"
            case["recovery_ms"] = round((time.monotonic() - started) * 1000, 3)
            assert len(provider.requests) == 2, provider.requests
            assert provider.requests[0]["raw_sha256"] == provider.requests[1]["raw_sha256"], \
                "automatic recovery rebuilt the frozen request"
            terminal.settle(0.3)
            assert len(provider.requests) == 2, "more than one replacement was sent"

            def durable_branch_complete():
                return next((value for value in sessions(terminal)
                             if (value["parent"] or {}).get("session_id") == terminal.session_id
                             and any(message["content"] == REPLY for message in value["messages"])), None)

            branch = wait(durable_branch_complete, terminal, timeout=5)
            saved = sessions(terminal)
            source = next(value for value in saved if value["id"] == terminal.session_id)
            assert len(saved) == 2, saved
            assert any(message["content"] == PARTIAL for message in source["messages"]), source
            assert source["messages"][0]["content"] == PROMPT
            assert branch["id"] != source["id"]
            assert branch["messages"][0]["content"] == PROMPT
            assert not any(message["content"] == PARTIAL for message in branch["messages"]), branch
            case["branch_id"] = branch["id"]
            case["source_partial_preserved"] = True
            case["frozen_request_sha256"] = provider.requests[0]["raw_sha256"]
        case["status"] = "passed"
    except Exception as error:
        case["status"], case["error"] = "failed", repr(error)
        if terminal:
            case["terminal"] = terminal.screen.text()
    finally:
        if directory and previous_mode is not None:
            directory.chmod(previous_mode)
        if terminal:
            if terminal.process.poll() is None:
                for _ in range(2):
                    terminal.write(b"\x1b")
                    terminal.settle(0.2)
                terminal.write(b"\x7f" * 200)
                terminal.settle(0.2)
            terminal.close()
            case["terminal_exit"] = terminal.process.returncode
            case["terminal_restored"] = b"\x1b[?1049l" in terminal.raw_tail
            case["sessions"] = sessions(terminal)
            if case["terminal_exit"] != 0 or not case["terminal_restored"]:
                case["status"] = "failed"
                case["cleanup_error"] = "terminal did not exit cleanly and restore the screen"
        provider.close()
        case["requests"] = provider.requests
        case["provider_errors"] = provider.failures
        if provider.failures:
            case["status"] = "failed"
    return case


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--failure-only", action="store_true",
                        help="Exercise only a real temporary session-directory write denial")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix="ca-recovery-branch-pty-", dir="/private/tmp")).resolve()
    os.chmod(root, 0o700)
    result = {"schema": "ca.recovery-branch-pty-receipt.v1", "status": "running", "root": str(root),
              "binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(), "cases": []}
    try:
        plans = [("source-save-failure", 100, 30, True)] if args.failure_only else [
            ("wide", 100, 30, False), ("narrow", 60, 24, False),
            ("source-save-failure", 100, 30, True)]
        for label, width, height, save_failure in plans:
            case = exercise(binary, root / label, width, height, save_failure)
            case["scenario"] = label
            result["cases"].append(case)
            if case["status"] != "passed":
                break
        result["status"] = "passed" if len(result["cases"]) == len(plans) and all(
            case["status"] == "passed" for case in result["cases"]) else "failed"
        assert hashlib.sha256(binary.read_bytes()).hexdigest() == result["binary_sha256"], \
            "candidate changed during validation"
    except Exception as error:
        result["status"], result["error"] = "failed", repr(error)
    finally:
        result["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(json.dumps(result, indent=2) + "\n")
        os.chmod(args.receipt, 0o600)
        print(json.dumps({"status": result["status"], "root": str(root), "error": result.get("error"),
                          "cases": [{key: case.get(key) for key in (
                              "scenario", "width", "height", "status", "recovery_ms", "save_failure_footer_observed", "error")}
                              for case in result["cases"]], "receipt": str(args.receipt)}), flush=True)
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
