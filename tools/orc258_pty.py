#!/usr/bin/env python3
"""Bounded, real TUI walkthroughs against a disposable persistent runtime.

python3 tools/orc258_pty.py --binary target/release/ca
Only a loopback fake provider is contacted. Every profile, workspace, session,
automation, recovery journal and failure injection belongs to the receipt root.
The parent environment is unchanged. No installed binary or live profile is used.
"""
import argparse
import contextlib
import datetime
import fcntl
import http.server
import hashlib
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import subprocess
import tempfile
import termios
import threading
import time
import uuid

from bug257_pty import Fixture, Screen


def wait(predicate, fixture=None, timeout=10):
    deadline = time.monotonic() + timeout
    while True:
        value = predicate()
        if value:
            return value
        if fixture:
            assert fixture.read(0.05), f"ca exited {fixture.process.returncode}: {fixture.screen.text()}"
        else:
            time.sleep(0.02)
        assert time.monotonic() < deadline, "bounded fixture wait timed out"


def content(message):
    raw = message.get("content", "")
    if isinstance(raw, str):
        return raw
    return "\n".join(item.get("text", "") for item in raw or [] if isinstance(item, dict))


class FakeProvider:
    def __init__(self):
        self.lock = threading.Lock()
        self.routes = {}
        self.after_tools = {}
        self.checkpoints = []
        self.handoffs = []
        self.requests = []
        provider = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_GET(self):
                payload = {"object": "list", "data": [{"id": "fixture-model", "object": "model"}]}
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps(payload).encode())

            def do_POST(self):
                length = int(self.headers.get("Content-Length", "0"))
                if length > 8 * 1024 * 1024:
                    self.send_error(413)
                    return
                request = json.loads(self.rfile.read(length))
                messages = request.get("messages", [])
                last_user = next((content(m) for m in reversed(messages) if m.get("role") == "user"), "")
                last = messages[-1] if messages else {}
                user_index = max((index for index, message in enumerate(messages) if message.get("role") == "user"), default=-1)
                last_tool = next((message for message in reversed(messages[user_index+1:]) if message.get("role") == "tool"), None)
                checkpoint = last_user.startswith("Create a compact checkpoint for the coding session transcript below.")
                handoff = last_user.startswith("Summarize the divergent source history for a user-approved switch")
                with provider.lock:
                    assert len(provider.requests) < 256, "provider fixture request ceiling exceeded"
                    provider.requests.append({"user": last_user[:65536], "last_role": last.get("role"),
                        "tool_id": last.get("tool_call_id"), "stream": request.get("stream", False),
                        "tools": [tool.get("function", {}).get("name") for tool in request.get("tools", [])],
                        "checkpoint": checkpoint, "repair": "only repair attempt" in last_user,
                        "handoff": handoff, "token_cap": request.get("max_tokens", request.get("max_completion_tokens")),
                        "tool_outputs": [{"id": message.get("tool_call_id"), "content": content(message)[:8192]}
                            for message in messages[user_index+1:] if message.get("role") == "tool"]})
                    response = next((value for marker, value in provider.routes.items() if marker in last_user), None)
                    if checkpoint:
                        assert provider.checkpoints, "unexpected compaction request"
                        response = provider.checkpoints.pop(0)
                    elif handoff:
                        assert provider.handoffs, "unexpected branch handoff request"
                        response = provider.handoffs.pop(0)
                    elif last_tool:
                        response = provider.after_tools.get(last_tool.get("tool_call_id")) or {"text": "ORC258_COMPLETE " + last_tool.get("tool_call_id", "fixture")}
                if checkpoint or handoff:
                    pass
                elif last_tool:
                    pass
                elif last_user.startswith("Revise plan ") and "Comments request revision only" in last_user:
                    revision = int(re.search(r"reviewed revision (\d+)", last_user).group(1))
                    response = {"calls": [("feedback-revision", "edit_plan", {"expected_revision": revision,
                        "body": "Revised selection phrase.\n\nThe requested feedback was applied."})]}
                response = response or {"text": "ORC258_FIXTURE_REPLY"}
                if response.get("delay"):
                    time.sleep(response["delay"])
                calls = response.get("calls", [])
                text = response.get("text", "")
                if request.get("stream"):
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream")
                    self.send_header("Cache-Control", "no-cache")
                    self.end_headers()
                    delta = {"role": "assistant"}
                    if calls:
                        delta["tool_calls"] = [{"index": index, "id": call_id, "type": "function",
                            "function": {"name": name, "arguments": json.dumps(arguments)}}
                            for index, (call_id, name, arguments) in enumerate(calls)]
                    else:
                        delta["content"] = text
                    chunk = {"id": "orc258-fixture", "object": "chat.completion.chunk", "model": "fixture-model",
                             "choices": [{"index": 0, "delta": delta, "finish_reason": None}]}
                    finish = {"id": "orc258-fixture", "object": "chat.completion.chunk", "model": "fixture-model",
                              "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls" if calls else "stop"}],
                              "usage": {"prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 120}}
                    try:
                        for value in [chunk, finish]:
                            self.wfile.write(("data: " + json.dumps(value) + "\n\n").encode())
                        self.wfile.write(b"data: [DONE]\n\n")
                        self.wfile.flush()
                    except (BrokenPipeError, ConnectionResetError):
                        pass
                else:
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    message = {"role": "assistant", "content": text}
                    if calls:
                        message["tool_calls"] = [{"id": call_id, "type": "function",
                            "function": {"name": name, "arguments": json.dumps(arguments)}}
                            for call_id, name, arguments in calls]
                    try:
                        self.wfile.write(json.dumps({"id": "orc258-fixture", "object": "chat.completion",
                            "model": "fixture-model", "choices": [{"index": 0, "message": message,
                            "finish_reason": "tool_calls" if calls else "stop"}],
                            "usage": {"prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 120}}).encode())
                    except (BrokenPipeError, ConnectionResetError):
                        pass

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}/v1"

    def arm(self, marker, calls=None, text=None):
        with self.lock:
            self.routes[marker] = {"calls": calls or [], "text": text or "ORC258_FIXTURE_REPLY"}

    def arm_after(self, tool_id, calls):
        with self.lock:
            self.after_tools[tool_id] = {"calls": calls}

    def arm_checkpoints(self, *responses):
        with self.lock:
            self.checkpoints = [{"text": response} if isinstance(response, str) else response for response in responses]

    def arm_handoffs(self, *responses):
        with self.lock:
            self.handoffs = [{"text": response} if isinstance(response, str) else response for response in responses]

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)


class Walkthrough(Fixture):
    def __init__(self, binary, root, provider, session=None, name="ORC258 primary", hooks=None):
        self.binary, self.root, self.provider = binary, root, provider
        self.workspace = root / "workspace"
        self.workspace.mkdir(exist_ok=True)
        self.runtime = root / "runtime"
        config = self.runtime / ".config/ca"
        config.mkdir(parents=True, exist_ok=True)
        configuration = ("trusted_workspaces = [" + json.dumps(str(self.workspace)) + "]\n"
            "[[permission_rules]]\ndecision='ask'\nkind='path'\ntool='read_file'\npath_pattern=" + json.dumps(str(self.workspace / "child-only.txt")) + "\n"
            "[providers.fixture]\nbase_url=" + json.dumps(provider.url) + "\napi_key=''\nmodels=['fixture-model']\n"
            "[defaults]\nprovider='fixture'\nmodel='fixture-model'\n"
            + ("[agent]\ntool_discovery='lazy'\n" if root.name == "discovery" else "") +
            "[harness_improvement]\nenabled=false\n[learning.dream]\nenabled=false\n[learning.distill]\nenabled=false\n")
        for hook in hooks or []:
            configuration += "\n[[hooks]]\n" + "".join(key + "=" + json.dumps(value) + "\n" for key, value in hook.items())
        (config / "config.toml").write_text(configuration)
        self.environment = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(self.runtime),
            "TERM": "xterm-256color", "LANG": "en_US.UTF-8", "COLORFGBG": "15;0"}
        self.screen = Screen(140, 45)
        self.bytes, self.raw_tail = 0, b""
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 45, 140, 0, 0))

        def terminal_session():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        arguments = [str(binary), "--profile", "default", "--yolo", "--exclude-workspace-context"]
        if session:
            arguments += ["--session", session]
            self.session_id = session
        else:
            self.session_id = str(uuid.uuid4())
            arguments += ["--new-session-id", self.session_id, "--name", name]
        self.process = subprocess.Popen(arguments, cwd=self.workspace, stdin=slave, stdout=slave, stderr=slave,
            env=self.environment, preexec_fn=terminal_session)
        os.close(slave)
        try:
            self.wait_text("fixture-model", 20)
        except BaseException:
            self.close()
            raise

    def settle(self, seconds=0.2):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            assert self.read(0.02), f"ca exited {self.process.returncode}"

    def command(self, value, expect=None, timeout=10):
        self.write(b"\x1b[200~" + value.encode() + b"\x1b[201~\r")
        if expect:
            self.wait_text(expect, timeout)
        else:
            self.settle()

    def dismiss(self):
        self.write(b"\x1b")
        self.settle()

    def page_tail(self):
        self.write(b"G" + b"k" * 12)
        self.settle()

    def saved(self):
        directory = self.runtime / ".local/share/ca/sessions"
        for path in directory.glob("*.json"):
            value = json.loads(path.read_text())
            record = value.get("session", value)
            if str(record.get("id", value.get("id"))) == self.session_id:
                return path, record
        return None

    def save(self):
        self.settle(0.4)
        previous = self.saved()
        stamp = previous[0].stat().st_mtime_ns if previous else None
        self.raw_tail = b""
        self.command("/save")
        def committed():
            current = self.saved()
            if current and (stamp is None or current[0].stat().st_mtime_ns != stamp or b"Saved:" in self.raw_tail):
                return current
            if "Session save failed:" in self.screen.text():
                raise AssertionError("fixture save failed: " + self.screen.text().split("Session save failed:")[-1].strip())
            return None
        return wait(committed, self)

    def select_plan_quote(self, quote):
        self.command("/plan show", "N comment selection")
        self.settle(0.2)
        lines = self.screen.text().splitlines()
        row, column = next((y, line.index(quote)) for y, line in enumerate(lines) if quote in line)
        self.write(f"\x1b[<0;{column+1};{row+1}M\x1b[<32;{column+len(quote)};{row+1}M\x1b[<0;{column+len(quote)};{row+1}m".encode())
        self.settle()
        self.write(b"n")
        self.wait_text("/plan comment selected")

    def cli(self, *args):
        result = subprocess.run([str(self.binary), *args], cwd=self.workspace, env=self.environment,
                                capture_output=True, text=True, timeout=20)
        assert result.returncode == 0, result.stderr
        return result.stdout


@contextlib.contextmanager
def preserve_clipboard(root, fixture):
    """Preserve macOS pasteboard types in AppleScript memory, never log them."""
    if not Path("/usr/bin/osascript").exists():
        raise RuntimeError("Plan selection fixture requires the macOS clipboard preservation helper")
    ready, done, restored = root / "clipboard-ready", root / "clipboard-done", root / "clipboard-restored"
    script = f'''set originalClipboard to the clipboard as record
do shell script "/usr/bin/touch {ready}"
try
    repeat 1800 times
        if (do shell script "if /bin/test -f {done}; then /usr/bin/printf done; fi") is "done" then exit repeat
        delay 0.1
    end repeat
end try
set the clipboard to originalClipboard
do shell script "/usr/bin/touch {restored}"
'''
    process = subprocess.Popen(["/usr/bin/osascript", "-e", script], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        wait(lambda: ready.exists() or process.poll() is not None, timeout=5)
        assert ready.exists() and process.poll() is None, "Clipboard preservation was unavailable; selection fixture stopped"
        yield
    finally:
        if fixture.process.poll() is None:
            fixture.close()
        done.touch()
        process.wait(timeout=20)
        assert process.returncode == 0 and restored.exists(), "Clipboard restoration failed"


def plan_case(fixture, provider):
    provider.arm("ORC258_PLAN_INITIAL", [("plan-initial", "propose_plan", {"expected_revision": 0,
        "title": "ORC258 review plan", "body": "Unique selection phrase.\n\nReview the fixture carefully.",
        "tasks": ["Inspect the fixture", "Verify the result"]})])
    fixture.command("/plan", "Plan mode")
    fixture.command("ORC258_PLAN_INITIAL", "ORC258_COMPLETE plan-initial", 15)
    fixture.select_plan_quote("Unique selection phrase")
    fixture.command("Make the validation explicit", "saved as a draft")
    fixture.command("/plan submit", "ORC258_COMPLETE feedback-revision", 15)
    fixture.command("/plan submit", "No draft comments")
    fixture.select_plan_quote("Revised selection phrase")
    fixture.command("This anchor must become stale", "saved as a draft")
    _, record = fixture.save()
    revision = record["work_state"]["plan"]["revision"]
    provider.arm("ORC258_PLAN_DRIFT", [("plan-drift", "edit_plan", {"expected_revision": revision,
        "body": "The previously selected paragraph was replaced."})])
    fixture.command("ORC258_PLAN_DRIFT", "ORC258_COMPLETE plan-drift", 15)
    fixture.command("/plan submit", "stale comments")
    _, record = fixture.save()
    notes = record["work_state"]["plan_feedback"]["notes"]
    assert sum(note["status"] == "submitted" for note in notes) == 1
    assert sum(note["status"] == "stale" for note in notes) == 1
    session = fixture.session_id
    fixture.close()
    resumed = Walkthrough(fixture.binary, fixture.root, provider, session=session)
    try:
        resumed.command("/plan comments", "This anchor must become stale")
        resumed.dismiss()
        resumed.command("/plan submit", "stale comments")
    finally:
        resumed.close()
    return {"selection_n": True, "submit_once": True, "stale_anchor": True, "resume_ledger": True}


def recovery_case(fixture, provider):
    target = fixture.workspace / "recorded.txt"
    target.write_text("original\n")
    provider.arm("ORC258_TYPED_EDIT", [("typed-edit", "write_file", {"path": "recorded.txt", "content": "agent edit\n"})])
    fixture.command("ORC258_TYPED_EDIT", "ORC258_COMPLETE typed-edit", 15)
    assert target.read_text() == "agent edit\n"
    fixture.save()
    journal_path = fixture.runtime / ".local/share/ca/recovery" / fixture.session_id / "journal.json"
    journal = json.loads(wait(lambda: journal_path.read_text() if journal_path.exists() else None, fixture))
    turn = next(group["turn_id"] for group in reversed(journal["groups"]) if group["records"])
    fixture.command("/recover", "Recorded turn edits")
    fixture.dismiss()
    fixture.command(f"/recover preview {turn}", "Confirmation:")
    token = wait(lambda: re.search(r"/recover restore ([0-9a-f]{12,64})", fixture.screen.text()), fixture).group(1)
    fixture.dismiss()
    target.write_text("external drift\n")
    fixture.command(f"/recover restore {token}", "failed")
    assert target.read_text() == "external drift\n"
    target.write_text("agent edit\n")
    session = fixture.session_id
    fixture.close()
    resumed = Walkthrough(fixture.binary, fixture.root, provider, session=session)
    try:
        _, before = wait(resumed.saved, resumed)
        before_path, _ = resumed.saved()
        before_metadata = json.loads(before_path.read_text())
        before_history = (before_metadata.get("message_count"), before_metadata.get("history_sha256"))
        resumed.command("/undo", "Undid", 15)
        assert target.read_text() == "original\n"
        resumed.command("/redo", "Redid", 15)
        assert target.read_text() == "agent edit\n"
        _, after = resumed.save()
        after_metadata = json.loads(resumed.saved()[0].read_text())
        assert (after_metadata.get("message_count"), after_metadata.get("history_sha256")) == before_history
        resumed.command(f"/recover preview {turn}", "Confirmation:")
        token = wait(lambda: re.search(r"/recover restore ([0-9a-f]{12,64})", resumed.screen.text()), resumed).group(1)
        resumed.dismiss()
        resumed.command(f"/recover restore {token}", "Recorded edits restored", 15)
        assert target.read_text() == "original\n"
        resumed.dismiss()
        resumed.command("/recover", "Recorded turn edits")
        assert "Redo" in resumed.raw_tail.decode("utf-8", "replace") or len(json.loads(journal_path.read_text())["groups"]) >= 2
    finally:
        resumed.close()
    return {"typed_edit_recorded": True, "hash_drift_rejected": True, "restart_undo_redo": True,
            "transcript_preserved": True, "restart_restore": True, "rescue_recorded": True}


def inventory_case(fixture, provider):
    provider.arm("ORC258_INVENTORY_SEED", text="ORC258_INVENTORY_SEED_REPLY")
    fixture.command("ORC258_INVENTORY_SEED", "ORC258_INVENTORY_SEED_REPLY", 15)
    path, record = fixture.save()
    initial = json.loads(path.read_text())
    legacy = dict(record)
    legacy["id"] = str(uuid.uuid4())
    legacy["title"] = "ORC258 unrelated legacy inventory fixture"
    legacy["messages"] = [{"role": "User", "content": "x" * (512 * 1024),
        "tool_calls": None, "thinking": None, "response_items": [],
        "timestamp": datetime.datetime.now(datetime.timezone.utc).isoformat()} for _ in range(18)]
    unrelated = path.parent / "orc258-unrelated-legacy.json"
    payload = json.dumps(legacy, separators=(",", ":")).encode()
    assert 8 * 1024 * 1024 < len(payload) < 64 * 1024 * 1024
    unrelated.write_bytes(payload)
    before_hash = hashlib.sha256(payload).hexdigest()
    # A real idle save first proves this inventory is cold and the short guard
    # refuses it. The submitted turn must then take the background preparation
    # path before the provider receives anything.
    fixture.command("/save", "Session save failed:", 10)
    assert "inventory" in fixture.screen.text().lower(), "cold inventory failure was not surfaced"
    marker, reply = "ORC258_COLD_INVENTORY_TURN", "ORC258_COLD_INVENTORY_REPLY"
    provider.arm(marker, text=reply)
    fixture.raw_tail = b""
    started = time.monotonic()
    fixture.write(b"\x1b[200~" + marker.encode() + b"\x1b[201~\r")
    fixture.wait_text(reply, 15)
    elapsed_ms = round((time.monotonic() - started) * 1000, 3)
    with provider.lock:
        requests = [request for request in provider.requests if marker in request["user"]]
    assert len(requests) == 1, "ownership preparation lost or duplicated the submitted turn"
    _, after = fixture.save()
    final = json.loads(path.read_text())
    assert str(after["id"]) == fixture.session_id
    assert final["message_count"] == initial["message_count"] + 2
    assert unrelated.read_bytes() == payload, "inventory preparation changed unrelated legacy history"
    # Visible key latency is measured after successful preparation/completion.
    # A fast warm-up cannot provide enough independent key samples to claim a
    # percentile specifically during the worker; do not label this that way.
    latency = fixture.latency(32)
    fixture.write(b"\x7f" * 32)
    wait(lambda: "abcdefghijklmnopqrstuvwxyzabcdef" not in fixture.screen.text(), fixture)
    return {"cold_short_guard_refused": True, "real_turn_completed": True,
        "provider_requests_for_turn": len(requests), "same_session_owner": True,
        "unrelated_bytes": len(payload), "unrelated_sha256": before_hash,
        "unrelated_bytes_unchanged": True, "turn_completion_ms": elapsed_ms,
        "keyboard_latency": latency,
        "latency_scope": "32 visible key samples after inventory warm-up and completed turn; no percentile claim during warm-up"}


def hook_cancellation_case(fixture, provider):
    first, second = "ORC258_HOOK_FIRST", "ORC258_HOOK_NEXT"
    provider.arm(first, text="ORC258_HOOK_FIRST_REPLY")
    provider.arm(second, text="ORC258_HOOK_NEXT_REPLY")
    first_started = fixture.workspace / "hook-first-started"
    first_finished = fixture.workspace / "hook-first-finished"
    next_finished = fixture.workspace / "hook-next-finished"
    fixture.command(first, "Running response hooks", 15)
    wait(first_started.exists, fixture, timeout=5)
    observed_start = time.monotonic()
    fixture.write(b"\x03")
    fixture.wait_text("Stopped", 5)
    fixture.write(b"\x1b[200~" + second.encode() + b"\x1b[201~")
    fixture.wait_text(second, 5)
    fixture.write(b"\r")
    fixture.wait_text("ORC258_HOOK_NEXT_REPLY", 10)
    wait(next_finished.exists, fixture, timeout=5)
    fixture.wait_text("Done", 5)
    draft = "ORC258_STALE_HOOK_GUARD_DRAFT"
    fixture.write(b"\x1b[200~" + draft.encode() + b"\x1b[201~")
    fixture.wait_text(draft, 5)
    # Observe beyond the old process's nominal finish boundary. The new draft
    # must survive and the cancelled hook cannot finish or restart provider work.
    while time.monotonic() - observed_start < 3.5:
        assert fixture.read(0.02), "fixture exited during stale-hook observation"
        assert draft in fixture.screen.text(), "old hook changed the next draft"
    assert not first_finished.exists(), "cancelled response hook still completed its process"
    with provider.lock:
        first_requests = [request for request in provider.requests if first in request["user"]]
        next_requests = [request for request in provider.requests if second in request["user"]]
    assert len(first_requests) == 1 and len(next_requests) == 1, "cancellation replayed or lost provider work"
    fixture.write(b"\x7f" * len(draft))
    wait(lambda: draft not in fixture.screen.text(), fixture)
    return {"post_response_hook_started": True, "single_ctrl_c": True,
        "draft_visible_before_enter": True, "next_enter_dispatched_once": True,
        "old_hook_process_cancelled": True, "later_draft_preserved": True,
        "first_requests": len(first_requests), "followup_requests": len(next_requests),
        "scope": "one owning pane; other-pane hook cancellation isolation remains covered by the focused state regression"}


def branch_handoff_checks(fixture, provider, source_path, target_path, source_id, target_id):
    provider.arm("ORC258_BRANCH_CHILD_DECISION", text="ORC258_CHILD_DECISION_REPLY")
    fixture.command("ORC258_BRANCH_CHILD_DECISION", "ORC258_CHILD_DECISION_REPLY")
    fixture.save()
    source_bytes, target_bytes = source_path.read_bytes(), target_path.read_bytes()
    source_metadata, target_metadata = json.loads(source_bytes), json.loads(target_bytes)
    start = len(provider.requests)
    provider.arm_handoffs("ORC258_CANCELLED_HANDOFF_CONTEXT", "", "ORC258_ACCEPTED_HANDOFF_CONTEXT")

    def preview(text):
        fixture.command("/branches summarize " + target_id, "Summarize and switch preview", 15)
        fixture.wait_text(text)
        token = wait(lambda: re.search(r"/branches accept ([0-9a-f]{64})", fixture.screen.text()), fixture).group(1)
        assert source_path.read_bytes() == source_bytes and target_path.read_bytes() == target_bytes
        fixture.dismiss()
        fixture.command("/pane list", "Session panes")
        fixture.wait_text(source_metadata["session"]["title"])
        fixture.dismiss()
        return token

    cancelled = preview("ORC258_CANCELLED_HANDOFF_CONTEXT")
    fixture.command("/branches cancel")
    fixture.command("/branches accept " + cancelled, "missing or stale")
    assert source_path.read_bytes() == source_bytes and target_path.read_bytes() == target_bytes
    fixture.command("/branches summarize " + target_id, "bounded, tool-free contract", 15)
    assert source_path.read_bytes() == source_bytes and target_path.read_bytes() == target_bytes
    accepted = preview("ORC258_ACCEPTED_HANDOFF_CONTEXT")
    fixture.command("/session", "branch handoff")
    assert re.search(r"branch handoff:\s*3", fixture.screen.text()), "handoff requests were not accounted separately"
    fixture.dismiss()
    fixture.command("/branches accept " + accepted, "Switched with the accepted branch summary", 15)
    fixture.session_id = target_id
    assert source_path.read_bytes() == source_bytes
    target_after = json.loads(target_path.read_text())
    assert (target_after["message_count"], target_after["history_sha256"]) == (target_metadata["message_count"], target_metadata["history_sha256"])
    handoffs = target_after["session"]["branch_handoffs"]["entries"]
    assert len(handoffs) == 1 and handoffs[0]["source_session_id"] == source_id and handoffs[0]["target_session_id"] == target_id
    assert handoffs[0]["through_message_count"] == source_metadata["message_count"]
    assert handoffs[0]["summary"] == "ORC258_ACCEPTED_HANDOFF_CONTEXT"
    assert len(handoffs[0]["source_prefix_sha256"]) == 64
    assert target_after["session"]["work_state"] == target_metadata["session"]["work_state"]
    requests = [request for request in provider.requests[start:] if request["handoff"]]
    assert len(requests) == 3 and all(not request["tools"] and request["token_cap"] == 2048 for request in requests)
    assert all("Source " + source_id + " target " + target_id in request["user"] for request in requests)
    return {"handoff_tool_free_bounded": True, "handoff_preview_keeps_source": True,
            "handoff_cancel_and_failure_preserve_both": True, "handoff_explicit_accept_switch": True,
            "handoff_target_provenance_only": True, "handoff_separate_usage": True}


def branches_case(fixture, provider):
    provider.arm("ORC258_BRANCH_SEED", text="ORC258_BRANCH_SEED_REPLY")
    fixture.command("ORC258_BRANCH_SEED", "ORC258_BRANCH_SEED_REPLY")
    path, record = fixture.save()
    fixture.command("/bookmark add Fixture checkpoint", "Bookmark saved")
    _, record = fixture.save()
    bookmark = record["bookmarks"]["entries"][0]["id"]
    source = fixture.session_id
    fixture.command(f"/bookmark branch {bookmark}", "Branch")
    # Locate the newly committed parent link without guessing a display index.
    directory = path.parent
    def child_record():
        for candidate in directory.glob("*.json"):
            value = json.loads(candidate.read_text()).get("session", {})
            if (value.get("parent") or {}).get("session_id") == source:
                return candidate, value
        return None
    child_path, child = wait(child_record, fixture)
    fixture.session_id = child["id"]
    fixture.command("/tree", "Branches ·")
    fixture.wait_text(source)
    assert child["id"] in fixture.screen.text()
    fixture.dismiss()
    # A moved committed source is a deterministic save failure in disposable data.
    moved = child_path.with_suffix(".fixture-backup")
    child_path.rename(moved)
    try:
        fixture.command("/bookmark add Must roll back", "Bookmark action failed")
        fixture.command("/tree switch " + source, "save failed")
        fixture.command("/pane list", "Session panes")
        fixture.wait_text(child["title"])
        fixture.dismiss()
    finally:
        moved.rename(child_path)
    handoff_checks = branch_handoff_checks(fixture, provider, child_path, path, child["id"], source)
    fixture.command("/resume")
    # The picker treats shifted letters as management shortcuts. A real paste
    # enters its search field without triggering Shift+R rename.
    fixture.write(b"\x1b[200~" + child["title"].encode() + b"\x1b[201~")
    fixture.settle()
    assert json.loads(child_path.read_text())["session"]["title"] == child["title"]
    fixture.write(b"A")
    fixture.wait_text("Archive saved session?")
    fixture.write(b"\x1b[C\r")
    wait(lambda: list((fixture.runtime / ".local/share/ca/archive").glob("*.json")), fixture)
    fixture.dismiss()
    fixture.command("/tree", "[archived]")
    return {"bookmark_branch": True, "tree_parent_uuid": True, "failed_save_keeps_source": True,
            "archive_visible": True, **handoff_checks}


def automation_case(fixture, provider):
    definition = {"schema": "ca.automation.v1", "id": "fixture-review", "name": "ORC258 reviewed automation",
        "automation_kind": {"kind": "standalone"}, "schedule": {"version": 2, "id": "fixture-review",
        "provider": "fixture", "model": "fixture-model", "workspace": str(fixture.workspace),
        "runtime_secs": 30, "output_bytes": 1024, "tools": [], "grants": [], "allow_network": False,
        "skills": [], "workspace_policy": "existing_read_only", "pinned_revision": None,
        "trigger": {"kind": "interval", "seconds": 3600, "anchor": None}, "missed_run_policy": "skip",
        "target": {"kind": "prompt", "prompt": "ORC258_AUTOMATION_RESULT"}, "delivery": {"kind": "none"}},
        "reasoning_effort": None, "pricing": {"kind": "local_zero"}, "context_sha256": [], "notification_policy": "muted"}
    draft = fixture.workspace / "automation.json"
    draft.write_text(json.dumps(definition))
    fixture.command("/automation preview automation.json", "Automation definition preview")
    fixture.page_tail()
    token = re.search(r"Exact confirmation: ([0-9a-f]{64})", fixture.screen.text()).group(1)
    fixture.dismiss()
    fixture.command("/automation create wrong-token", "does not match")
    definition["name"] = "Unreviewed file drift"
    draft.write_text(json.dumps(definition))
    fixture.command(f"/automation create {token}", "created and enabled")
    fixture.dismiss()
    records = json.loads(fixture.cli("automation", "--json", "list"))["automations"]
    assert records[0]["name"] == "ORC258 reviewed automation"
    fixture.command("/automation pause fixture-review", "paused")
    fixture.dismiss()
    fixture.command("/automation resume fixture-review", "resumed")
    fixture.dismiss()
    provider.arm("ORC258_AUTOMATION_METADATA", [("metadata-list", "list_local_automations", {"limit":25})])
    fixture.command("ORC258_AUTOMATION_METADATA", "ORC258_COMPLETE metadata-list", 15)
    metadata_output = next(output["content"] for request in reversed(provider.requests)
        for output in request["tool_outputs"] if output["id"] == "metadata-list")
    metadata = json.loads(metadata_output)
    assert metadata["automations"][0]["id"] == "fixture-review"
    assert "schedule" not in metadata_output and "workspace" not in metadata_output
    conversational = json.loads(json.dumps(definition))
    conversational["id"] = conversational["schedule"]["id"] = "fixture-conversation"
    conversational["name"] = "ORC258 conversational draft"
    provider.arm("ORC258_CONVERSATIONAL_AUTOMATION", [("automation-proposal", "propose_automation", {"definition": conversational})])
    fixture.command("ORC258_CONVERSATIONAL_AUTOMATION", "Automation definition preview", 15)
    assert len(json.loads(fixture.cli("automation", "--json", "list"))["automations"]) == 1, "conversational proposal created without confirmation"
    fixture.page_tail()
    conversation_token = wait(lambda: re.search(r"Exact confirmation: ([0-9a-f]{64})", fixture.screen.text()), fixture).group(1)
    fixture.dismiss()
    fixture.command(f"/automation create {conversation_token}", "created and enabled")
    fixture.dismiss()
    assert len(json.loads(fixture.cli("automation", "--json", "list"))["automations"]) == 2
    fixture.command(f"/automation create {conversation_token}", "Preview the exact definition")
    fixture.command("/automation run fixture-review", "finished", 20)
    fixture.dismiss()
    fixture.command("/automation history fixture-review", "Succeeded")
    fixture.dismiss()
    history = json.loads(fixture.cli("automation", "--json", "history", "fixture-review"))["runs"]
    run_id = next(event["run_id"] for event in reversed(history) if event["state"] == "succeeded")
    fixture.command(f"/automation result fixture-review {run_id}", "Recorded automation result")
    fixture.wait_text("ORC258_FIXTURE_REPLY")
    fixture.dismiss()
    fixture.command("/automation delete fixture-review", "Automation deletion preview")
    fixture.page_tail()
    delete_token = wait(lambda: re.search(r"Exact deletion confirmation: ([0-9a-f]{64})", fixture.screen.text()), fixture).group(1)
    fixture.dismiss()
    fixture.command("/automation delete fixture-review wrong-token", "does not match")
    fixture.command(f"/automation delete fixture-review {delete_token}", "deleted; immutable run receipts retained")
    fixture.dismiss()
    after_delete = json.loads(fixture.cli("automation", "--json", "history", "fixture-review"))["runs"]
    assert after_delete == history, "deleting the definition modified immutable receipts"
    fixture.command(f"/automation delete fixture-review {delete_token}", "Preview deletion")
    fixture.command("/automation result fixture-review", "Resumed:", 15)
    return {"preview_exact_confirmation": True, "source_drift_frozen": True, "pause_resume": True,
            "conversational_preview": True, "conversational_confirmation_once": True, "safe_model_metadata": True,
            "run_history": True, "private_result_inspection": True, "delete_confirmation_once": True,
            "delete_retains_receipts": True, "result_navigation": True}


def dashboard_case(fixture, provider):
    # Each dashboard belongs to its pane. Keep both panes wide enough to show
    # the actual roster-and-preview user path after a vertical split.
    fixture.resize(280, 45)
    fixture.settle()
    (fixture.workspace / "child-only.txt").write_text("child fixture\n")
    provider.arm("ORC258_CHILD_PARENT", [("child-spawn", "spawn_agent", {"task": "ORC258_CHILD_WAIT",
        "tools": ["read_file"]})])
    provider.arm("ORC258_CHILD_WAIT", [("child-read", "read_file", {"path": "child-only.txt"})])
    fixture.command("ORC258_CHILD_PARENT", "ORC258_COMPLETE child-spawn", 15)
    fixture.save()
    fixture.command("/split vertical new")
    fixture.command("/pane list", "Session panes")
    fixture.dismiss()
    fixture.command("/dashboard", "Needs input", 15)
    fixture.wait_text("ORC258_CHILD_WAIT")
    # Select by preview identity, not by a fixed changing row number.
    for _ in range(16):
        right = "\n".join(line[217:] for line in fixture.screen.text().splitlines())
        if "ORC258_CHILD_WAIT" in right:
            break
        fixture.write(b"\x1b[B")
        fixture.settle(0.06)
    else:
        raise AssertionError("child preview was not selectable")
    fixture.write(b"\r")
    # Wait for the owning pane's agent controls, not the same task text still
    # visible in the outgoing dashboard preview.
    fixture.wait_text("↑/↓ select · Enter details", 10)
    fixture.write(b"m")
    fixture.wait_text("message>", 10)
    fixture.write(b"ORC258_REPLY_FROM_OWNER\r")
    fixture.wait_text("Agent action completed", 10)
    fixture.write(b"c")
    fixture.wait_text("Confirm cancellation", 10)
    fixture.write(b"\x1b[C\r")
    fixture.wait_text("Cancelled", 10)
    # Esc may first cancel an asynchronous session operation. Confirm that
    # the agent controls actually close before entering a composer command.
    for _ in range(3):
        fixture.dismiss()
        if "↑/↓ select · Enter details" not in fixture.screen.text():
            break
    else:
        raise AssertionError("agent controls did not close after bounded Esc attempts")
    fixture.command("/pane list", "* pane 1")
    fixture.dismiss()
    fixture.settle(0.5)
    metadata_path, _ = fixture.saved()
    metadata_backup = metadata_path.with_suffix(".pane-close-fixture-backup")
    metadata_path.rename(metadata_backup)
    try:
        fixture.command("/pane close", "Pane save failed; pane kept open", 15)
        fixture.command("/pane list", "Session panes")
        fixture.wait_text("* pane 1")
        fixture.wait_text("pane 2")
        fixture.dismiss()
    finally:
        metadata_backup.rename(metadata_path)
    # A second real process publishes bounded observation metadata only.
    other = Walkthrough(fixture.binary, fixture.root, provider, name="ORC258 external private title")
    try:
        other.command("ORC258_EXTERNAL_PRIVATE_PROMPT", "ORC258_FIXTURE_REPLY")
        other.save()
        fixture.command("/dashboard")
        fixture.wait_text(f"PID {other.process.pid}", 15)
        for _ in range(30):
            right = "\n".join(line[77:139] for line in fixture.screen.text().splitlines())
            if "External activity is read-only" in right or "observation" in right:
                break
            fixture.write(b"\x1b[B")
            fixture.settle(0.06)
        else:
            raise AssertionError("external observation preview was not selectable")
        fixture.wait_text("Last committed session transcript", 15)
        fixture.wait_text("ORC258_FIXTURE_REPLY", 15)
        fixture.write(b"\r")
        fixture.settle()
        assert "New Agent" in fixture.screen.text(), "external activation changed focus"
        presence = fixture.runtime / ".config/ca/dashboard-presence"
        values = [path.read_text() for path in presence.glob("*.json")]
        assert values and all("PRIVATE_PROMPT" not in value and "private title" not in value for value in values)
    finally:
        other.close()
    return {"all_local_panes": True, "needs_input_child": True, "owning_pane_focus": True,
            "reply_stop": True, "reply_acknowledged": True, "agent_controls_closed": True,
            "pane_close_save_failure_keeps_owner": True,
            "external_read_only": True, "last_committed_preview": True, "private_metadata": True}


def compaction_case(fixture, provider):
    for index in range(3):
        marker = f"ORC258_COMPACT_SEED_{index}"
        provider.arm(marker, text=marker + "_REPLY")
        fixture.command(marker, marker + "_REPLY")
        fixture.settle(0.3)
    fixture.save()
    sections = ["Task", "Constraints", "Facts", "Hypotheses", "Files Touched", "Decisions", "Test State", "Open Issues"]
    valid = "\n".join(f"## {section}\n" + ("ORC258_REPAIRED_CHECKPOINT" if section == "Facts" else "(none)") + "\n" for section in sections)
    start = len(provider.requests)
    provider.arm_checkpoints("Nonempty but invalid initial summary", valid)
    fixture.command("/compact here 1", "Context checkpoint created", 15)
    _, saved = fixture.save()
    checkpoint = saved["context_checkpoint"]
    assert checkpoint["format_version"] == 2 and "ORC258_REPAIRED_CHECKPOINT" in checkpoint["summary"]
    requests = [request for request in provider.requests[start:] if request["checkpoint"]]
    assert len(requests) == 2 and not requests[0]["repair"] and requests[1]["repair"]
    original_prompt = requests[1]["user"].split("\n\nHost quality validation rejected the previous candidate:")[0]
    assert original_prompt == requests[0]["user"], "repair substituted different source"
    start = len(provider.requests)
    provider.arm_checkpoints("Invalid exhausted initial summary", "Invalid exhausted repaired summary")
    fixture.command("/compact here 1", "Compaction failed:", 15)
    _, failed_saved = fixture.save()
    assert failed_saved["context_checkpoint"] == checkpoint
    failed_requests = [request for request in provider.requests[start:] if request["checkpoint"]]
    assert len(failed_requests) == 2 and failed_requests[1]["repair"]
    start = len(provider.requests)
    provider.arm_checkpoints({"text": valid.replace("REPAIRED", "CANCELLED"), "delay": 2})
    fixture.command("/compact here 1", "Creating context checkpoint", 10)
    wait(lambda: any(request["checkpoint"] for request in provider.requests[start:]), fixture)
    fixture.write(b"\x1b")
    fixture.wait_text("Compaction cancelled", 10)
    fixture.settle(2.4)
    _, cancelled_saved = fixture.save()
    assert cancelled_saved["context_checkpoint"] == checkpoint
    return {"manual_compaction": True, "invalid_first_valid_repair": True, "separate_request_receipts": True,
            "frozen_repair_source": True, "one_repair_limit": True, "previous_checkpoint_kept_after_exhaustion": True,
            "escape_cancels_delayed_checkpoint": True, "previous_checkpoint_kept_after_cancellation": True}


def discovery_case(fixture, provider):
    start = len(provider.requests)
    provider.arm("ORC258_LAZY_DISCOVERY", [("discovered-list-agents", "search_tools", {"query": "list_agents", "limit": 1})])
    provider.arm_after("discovered-list-agents", [("activated-list-agents", "list_agents", {})])
    fixture.command("ORC258_LAZY_DISCOVERY", "ORC258_COMPLETE activated-list-agents", 15)
    requests = provider.requests[start:]
    assert "search_tools" in requests[0]["tools"] and "list_agents" not in requests[0]["tools"]
    activated = next(request for request in requests[1:] if any(output["id"] == "discovered-list-agents" for output in request["tool_outputs"]))
    assert "list_agents" in activated["tools"], "discovered tool missing on later provider request"
    assert any(any(output["id"] == "activated-list-agents" for output in request["tool_outputs"]) for request in requests)
    return {"lazy_initial_schema": True, "search_owned_batch": True, "activated_only_later_request": True,
            "discovered_tool_executed": True}


CASES = {"plan": plan_case, "recovery": recovery_case, "branches": branches_case,
         "dashboard": dashboard_case, "automation": automation_case,
         "compaction": compaction_case, "discovery": discovery_case,
         "inventory": inventory_case, "hook-cancellation": hook_cancellation_case}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--case", action="append", choices=list(CASES))
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix="ca-orc258-pty-", dir="/private/tmp")).resolve()
    provider = FakeProvider()
    with binary.open("rb") as executable:
        digest = hashlib.sha256()
        for block in iter(lambda: executable.read(1024 * 1024), b""):
            digest.update(block)
        binary_sha256 = digest.hexdigest()
    receipt = {"status": "running", "root": str(root), "binary": str(binary), "binary_sha256": binary_sha256, "cases": {},
               "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    try:
        for name in args.case or CASES:
            case_root = root / name
            case_root.mkdir()
            fixture = None
            try:
                hooks = [{"event": "after_agent_response", "enabled": True,
                    "fail_closed": False, "timeout_secs": 8,
                    "command": "if [ ! -e hook-first-started ]; then /usr/bin/touch hook-first-started; /bin/sleep 3; /usr/bin/touch hook-first-finished; else /usr/bin/touch hook-next-started; /bin/sleep 1; /usr/bin/touch hook-next-finished; fi"}] if name == "hook-cancellation" else None
                fixture = Walkthrough(binary, case_root, provider, hooks=hooks)
                if name == "plan":
                    with preserve_clipboard(case_root, fixture):
                        checks = CASES[name](fixture, provider)
                else:
                    checks = CASES[name](fixture, provider)
                receipt["cases"][name] = {"status": "passed", "checks": checks, "output_bytes": fixture.bytes}
            except Exception as error:
                receipt["cases"][name] = {"status": "failed", "error": str(error),
                    "screen": fixture.screen.text() if fixture else "startup failed"}
            finally:
                if fixture and fixture.process.poll() is None:
                    fixture.close()
                (root / "result.json").write_text(json.dumps(receipt, indent=2) + "\n")
        receipt["status"] = "passed" if all(case["status"] == "passed" for case in receipt["cases"].values()) else "failed"
        receipt["provider_requests"] = len(provider.requests)
        receipt["provider_request_receipts"] = provider.requests
        receipt["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    finally:
        provider.close()
        (root / "result.json").write_text(json.dumps(receipt, indent=2) + "\n")
        print(json.dumps({"status": receipt["status"], "root": str(root), "result": str(root / "result.json"),
            "binary_sha256": binary_sha256, "cases": {name: {key: (value[:600] if key == "error" else value)
            for key, value in case.items() if key != "screen"} for name, case in receipt["cases"].items()}}), flush=True)
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
