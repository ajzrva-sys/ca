#!/usr/bin/env python3
"""Actual cross-provider plan workflow and graph proof with disposable state.

python3 tools/plan_workflow_pty.py --binary target/debug/ca --output scratch/plan-workflow/run-1
Only credentialless loopback Chat Completions and Responses fixtures are used.
Retains the actual provider requests, saved workflow, Markdown, terminal frames,
binary identity and terminal restoration; does not load the user's CA profile.
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
import signal
import struct
import subprocess
import tempfile
import termios
import threading
import time
import uuid

from bug257_pty import Screen
from orc258_pty import Walkthrough, content, wait


PROMPT = "PLAN_WORKFLOW_FIXTURE: Create, review, revise and review the plan across A, B, A and C, then implement the accepted plan on D."
PROOF = "PLAN_WORKFLOW_IMPLEMENTED_ON_D\n"
TITLE = "Cross-provider fixture plan"
AUTHOR = "AUTHOR_A_V1"
FEEDBACK = "REVIEW_B_REQUIRES_EVIDENCE_CHECK"
REVISION = "REVISION_A_V2_INCLUDES_EVIDENCE_CHECK"
ACCEPTED = "REVIEW_C_ACCEPTS_EXACT_V2"
FINAL = "WORKFLOW_IMPLEMENTATION_D_COMPLETE"
MODEL = {route: "workflow-" + route for route in ("orchestrator", "a", "b", "c", "d")}
PROTOCOL = {route: "openai_responses" if route in ("b", "d") else "chat_completions" for route in MODEL}
MUTATING_TOOLS = {"write_file", "edit_file", "delete_file", "run_command", "shell", "spawn_agent", "task", "apply_patch"}


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def functions(payload):
    return [tool.get("function", tool).get("name") for tool in payload.get("tools", [])]


def message_text(payload):
    return "\n".join(str(message.get("output", "")) if message.get("type") == "function_call_output"
                     else content(message) for message in payload.get("messages", payload.get("input", [])))


def tool_result(payload, call_id):
    for message in payload.get("messages", payload.get("input", [])):
        if message.get("tool_call_id", message.get("call_id")) == call_id and (
                message.get("role") == "tool" or message.get("type") == "function_call_output"):
            return message.get("output", content(message))
    return None


def start_arguments():
    return {"action": "start", "goal": PROMPT,
        "participants": [{"id": lane, "route": {"provider": lane, "model": MODEL[lane]}}
                         for lane in ("a", "b", "c", "d")],
        "steps": [
            {"participant": "a", "role": "author", "instructions": AUTHOR},
            {"participant": "b", "role": "review", "instructions": FEEDBACK},
            {"participant": "a", "role": "revise", "instructions": REVISION},
            {"participant": "c", "role": "review", "instructions": ACCEPTED}],
        "implementation": {"participant": "d", "role": "implement", "instructions": FINAL}}


class Provider:
    """Strict route fixture: each physical request is recorded before response."""
    def __init__(self):
        self.lock = threading.Lock()
        self.requests, self.errors = [], []
        self.plan_stages = []
        self.start = start_arguments()
        provider = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_GET(self):
                lane = self.path.split("/")[1]
                if lane not in MODEL or self.path != f"/{lane}/v1/models":
                    self.send_error(404)
                    return
                self.reply_json({"object": "list", "data": [{"id": MODEL[lane], "object": "model"}]})

            def reply_json(self, value):
                body = json.dumps(value).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("x-request-id", "plan-workflow-fixture")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def do_POST(self):
                try:
                    lane = self.path.split("/")[1]
                    assert lane in MODEL, "unconfigured route reached fixture"
                    expected = "responses" if PROTOCOL[lane] == "openai_responses" else "chat/completions"
                    assert self.path == f"/{lane}/v1/{expected}", (lane, self.path)
                    length = int(self.headers.get("Content-Length", "0"))
                    assert 0 < length <= 8 * 1024 * 1024, "unbounded fixture request"
                    raw = self.rfile.read(length)
                    payload = json.loads(raw)
                    assert payload.get("model") == MODEL[lane], "route/model mismatch"
                    with provider.lock:
                        assert len(provider.requests) < 64, "request bound exceeded"
                        provider.requests.append({"lane": lane, "path": self.path,
                            "body": payload, "sha256": hashlib.sha256(raw).hexdigest()})
                        calls, text = provider.response(lane, payload)
                    self.respond(lane, payload, calls, text)
                except (BrokenPipeError, ConnectionResetError):
                    pass
                except Exception as error:
                    with provider.lock:
                        provider.errors.append(repr(error))
                    self.send_error(500, str(error)[:200])

            def respond(self, lane, payload, calls, text):
                if PROTOCOL[lane] == "openai_responses":
                    output = [{"id": "fc-" + call_id, "type": "function_call", "call_id": call_id,
                               "name": name, "arguments": json.dumps(arguments), "status": "completed"}
                              for call_id, name, arguments in calls]
                    if text:
                        output.append({"id": "msg-fixture", "type": "message", "role": "assistant",
                            "status": "completed", "content": [{"type": "output_text", "text": text, "annotations": []}]})
                    response = {"id": "resp-workflow", "object": "response", "status": "completed",
                        "model": MODEL[lane], "output": output,
                        "usage": {"input_tokens": 20, "output_tokens": 10, "total_tokens": 30}}
                    events = []
                    for index, item in enumerate(output):
                        if item["type"] == "function_call":
                            events += [{"type": "response.output_item.added", "output_index": index,
                                "item": {**item, "arguments": ""}},
                                {"type": "response.function_call_arguments.done", "output_index": index,
                                 "arguments": item["arguments"]},
                                {"type": "response.output_item.done", "output_index": index, "item": item}]
                        else:
                            events.append({"type": "response.output_text.delta", "output_index": index,
                                           "content_index": 0, "delta": text})
                    events.append({"type": "response.completed", "response": response})
                else:
                    message = {"role": "assistant", "content": text}
                    if calls:
                        message["tool_calls"] = [{"id": call_id, "type": "function", "function": {
                            "name": name, "arguments": json.dumps(arguments)}} for call_id, name, arguments in calls]
                    response = {"id": "chat-workflow", "object": "chat.completion", "model": MODEL[lane],
                        "choices": [{"index": 0, "message": message, "finish_reason": "tool_calls" if calls else "stop"}],
                        "usage": {"prompt_tokens": 20, "completion_tokens": 10, "total_tokens": 30}}
                    delta = {"role": "assistant", "content": text}
                    if calls:
                        delta["tool_calls"] = [{"index": index, "id": call_id, "type": "function",
                            "function": {"name": name, "arguments": json.dumps(arguments)}}
                            for index, (call_id, name, arguments) in enumerate(calls)]
                    events = [{"choices": [{"index": 0, "delta": delta, "finish_reason": None}]},
                        {"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls" if calls else "stop"}],
                         "usage": response["usage"]}]
                if not payload.get("stream"):
                    self.reply_json(response)
                    return
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("x-request-id", "plan-workflow-fixture")
                self.send_header("Connection", "close")
                self.end_headers()
                for event in events:
                    self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
                if PROTOCOL[lane] == "chat_completions":
                    self.wfile.write(b"data: [DONE]\n\n")
                self.wfile.flush()
                self.close_connection = True

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}"

    def response(self, lane, payload):
        names = functions(payload)
        if lane == "orchestrator":
            if tool_result(payload, "start-workflow") is None:
                assert "plan_workflow" in names, "plan_workflow is absent from the actual tool catalog"
                return [("start-workflow", "plan_workflow", self.start)], ""
            return [], "WORKFLOW_ORCHESTRATOR_DONE"
        text = message_text(payload)
        if lane == "d":
            assert "write_file" in names, "implementation did not receive fresh Agent tools"
            assert REVISION in text, "implementation omitted the revised plan"
            if tool_result(payload, "implement-write") is None:
                return [("implement-write", "write_file", {"path": "workflow-proof.txt", "content": PROOF})], ""
            if tool_result(payload, "implement-read") is None:
                return [("implement-read", "read_file", {"path": "workflow-proof.txt"})], ""
            assert PROOF.strip() in str(tool_result(payload, "implement-read")), "implementation omitted actual readback"
            if tool_result(payload, "implement-check") is None:
                return [("implement-check", "run_command", {"command": "cat workflow-proof.txt"})], ""
            event = re.search(r"recorded_event_id:\s*([0-9a-f-]{36})", str(tool_result(payload, "implement-check")))
            assert event, "implementation command omitted its recorded evidence ID"
            task = re.search(r"([0-9a-f-]{36}): Write and read back the provider D proof file", text)
            revisions = re.findall(r"(?:Current revision:|updated to revision)\s*(\d+)", text)
            assert task and revisions, "implementation context omitted stable task and revision"
            if tool_result(payload, "implement-evidence") is None:
                return [("implement-evidence", "record_plan_evidence", {"expected_revision": int(revisions[-1]),
                    "task_id": task.group(1), "event_id": event.group(1), "note": "Actual provider D write/read/command proof"})], ""
            evidence_revision = re.search(r"updated to revision\s*(\d+)", str(tool_result(payload, "implement-evidence")))
            assert evidence_revision, "native evidence was not attached: " + str(tool_result(payload, "implement-evidence"))
            if tool_result(payload, "implement-complete") is None:
                return [("implement-complete", "update_plan_task", {"expected_revision": int(evidence_revision.group(1)),
                    "task_id": task.group(1), "status": "completed"})], ""
            assert "updated to revision" in str(tool_result(payload, "implement-complete")), "native completion was not committed"
            return [], FINAL
        assert not MUTATING_TOOLS.intersection(names), ("planning exposed mutating tools", names)
        assert "report_structured_output" in names and "read_file" in names, names
        if lane == "a":
            visit = "a2" if REVISION in text and FEEDBACK in text else "a1"
        else:
            visit = lane
        if visit == "a2":
            assert AUTHOR in text and FEEDBACK in text, "returning participant lost own output or feedback"
        if lane == "c":
            assert REVISION in text, "reviewer C did not receive revision two"
        read_id = "read-" + visit
        if tool_result(payload, read_id) is None:
            return [(read_id, "read_file", {"path": "evidence.txt"})], ""
        assert "REPOSITORY_EVIDENCE" in str(tool_result(payload, read_id)), "planning readback missing"
        if visit not in self.plan_stages:
            self.plan_stages.append(visit)
        if visit in ("a1", "a2"):
            marker = AUTHOR if visit == "a1" else REVISION
            value = {"title": TITLE, "body": "# " + TITLE + "\n\n" + marker + "\n\nWrite workflow-proof.txt, then read it back.\n",
                     "tasks": ["Write and read back the provider D proof file"]}
        else:
            value = {"verdict": "needs_changes" if lane == "b" else "accepted",
                     "feedback": FEEDBACK if lane == "b" else ACCEPTED}
        return [("report-" + visit, "report_structured_output", {"value": value})], ""

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)


class Terminal(Walkthrough):
    def __init__(self, binary, root, provider):
        self.binary, self.root, self.provider = binary, root, provider
        self.workspace = root / "workspace"
        self.workspace.mkdir()
        (self.workspace / "evidence.txt").write_text("REPOSITORY_EVIDENCE\n")
        self.runtime = root / "runtime"
        config = self.runtime / ".config/ca"
        config.mkdir(parents=True)
        configuration = "trusted_workspaces = [" + json.dumps(str(self.workspace)) + "]\n"
        for lane in MODEL:
            configuration += (f"[providers.{lane}]\nbase_url=" + json.dumps(f"{provider.url}/{lane}/v1")
                + "\napi_key=''\nmodels=" + json.dumps([MODEL[lane]]) + "\nprotocol=" + json.dumps(PROTOCOL[lane]) + "\n")
        configuration += ("[defaults]\nprovider='orchestrator'\nmodel='workflow-orchestrator'\n"
            "[harness_improvement]\nenabled=false\n[learning.dream]\nenabled=false\n[learning.distill]\nenabled=false\n")
        self.config = config / "config.toml"
        self.config.write_text(configuration)
        self.config_sha256 = digest(self.config)
        self.environment = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(self.runtime),
            "XDG_CONFIG_HOME": str(self.runtime / ".config"), "XDG_DATA_HOME": str(self.runtime / ".local/share"),
            "XDG_CACHE_HOME": str(self.runtime / ".cache"), "TERM": "xterm-256color", "LANG": "en_US.UTF-8", "COLORFGBG": "15;0"}
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
        self.process = subprocess.Popen([str(binary), "--profile", "default", "--yolo", "--exclude-workspace-context",
            "--new-session-id", self.session_id, "--name", "Cross-provider plan workflow fixture"], cwd=self.workspace,
            stdin=slave, stdout=slave, stderr=slave, env=self.environment, preexec_fn=terminal_session)
        os.close(slave)
        try:
            self.wait_text(MODEL["orchestrator"], 20)
        except BaseException:
            self.close()
            raise

    def read(self, timeout=0.05):
        before = self.bytes
        alive = super().read(timeout)
        count = self.bytes - before
        if count:
            self.raw.extend(self.raw_tail[-count:])
        return alive

    def close(self):
        if self.process.poll() is None:
            for _ in range(3):
                self.write(b"\x1b")
                deadline = time.monotonic() + 0.15
                while self.process.poll() is None and time.monotonic() < deadline:
                    self.read(0.02)
            self.write(b"\x1b[200~/quit\x1b[201~\r")
            deadline = time.monotonic() + 2
            while self.process.poll() is None and time.monotonic() < deadline:
                self.read(0.05)
        if self.process.poll() is None:
            self.write(b"\x03")
            deadline = time.monotonic() + 5
            second_interrupt = time.monotonic() + 1
            while self.process.poll() is None and time.monotonic() < deadline:
                self.read(0.05)
                if time.monotonic() >= second_interrupt:
                    self.write(b"\x03")
                    second_interrupt = deadline + 1
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                os.killpg(self.process.pid, signal.SIGKILL)
        self.process.wait(timeout=5)
        self.restored_termios = termios.tcgetattr(self.master) == self.original_termios
        os.close(self.master)


def scenario(terminal, provider, output):
    terminal.command(PROMPT)
    proof = terminal.workspace / "workflow-proof.txt"

    def implemented():
        assert not provider.errors, provider.errors
        record = terminal.saved()
        workflow = record[1].get("work_state", {}).get("plan_workflow") if record else None
        assert not workflow or workflow["status"] not in ("failed", "cancelled"), workflow
        return proof.exists() and proof.read_text() == PROOF

    wait(implemented, terminal, timeout=60)
    wait(lambda: any(entry["lane"] == "d" and tool_result(entry["body"], "implement-complete") is not None
                     for entry in provider.requests), terminal, timeout=20)
    terminal.wait_text(FINAL, 20)
    terminal.settle(0.5)
    assert not provider.errors, provider.errors
    assert provider.plan_stages == ["a1", "b", "a2", "c"], provider.plan_stages
    terminal.save()
    path, saved = terminal.saved()
    work = saved["work_state"]
    workflow = work.get("plan_workflow")
    assert isinstance(workflow, dict), "workflow missing from native saved work state"
    assert workflow["status"] == "completed", workflow
    steps, snapshots = workflow["steps"], workflow["snapshots"]
    assert [step["participant"] for step in steps] == ["a", "b", "a", "c", "d"], steps
    assert [step["role"] for step in steps] == ["author", "review", "revise", "review", "implement"], steps
    assert all(step["status"] == "completed" for step in steps), steps
    assert len({step["id"] for step in steps}) == 5, "return visits reused a ledger identity"
    assert all(step["route"] == {"provider": step["participant"], "model": MODEL[step["participant"]]}
               for step in steps), "saved actual routes differ"
    assert len(snapshots) == 3 and AUTHOR in snapshots[0]["markdown"] and REVISION in snapshots[1]["markdown"], snapshots
    assert snapshots[2]["plan"]["tasks"][0]["status"] == "completed", "implementation snapshot omitted verified task state"
    assert steps[1]["feedback"] == FEEDBACK and steps[3]["feedback"] == ACCEPTED, steps
    assert steps[3]["accepted"] is True and workflow["accepted_plan"] == steps[3]["source_plan"], workflow
    assert steps[3]["source_plan"]["revision"] == snapshots[1]["plan"]["revision"], "acceptance bound to wrong revision"
    assert work["plan"]["tasks"][0]["status"] == "completed", "typed task completion was not committed"
    assert work["plan"]["tasks"][0]["evidence"], "completed plan lacks native command evidence"
    (output / "saved-work-state.json").write_text(json.dumps(work, indent=2) + "\n")
    markdown = list((terminal.runtime / ".local/share/ca").rglob("plan.md"))
    assert markdown, "latest plan Markdown was not published"
    assert any(REVISION in file.read_text() for file in markdown), "plan.md is stale"
    revisions = list(markdown[0].parent.joinpath("revisions").glob("*.md"))
    assert len(revisions) == len(snapshots), "immutable plan revision files were omitted"
    for index, revision in enumerate(sorted(revisions)):
        assert revision.read_text() == snapshots[index]["markdown"], "Markdown differs from immutable structured snapshot"
        (output / revision.name).write_bytes(revision.read_bytes())
    terminal.command("/plan graph")
    terminal.settle(0.3)
    wide = terminal.screen.text()
    assert "workflow-a" in wide and "workflow-b" in wide, "handoff graph omitted actual routes: " + wide
    (output / "graph-wide.txt").write_text(wide)
    terminal.write(b"\x1b[H\x1b[B\x1b[B\r")
    terminal.settle(0.3)
    selected = terminal.screen.text()
    assert REVISION in selected, "selected revision is absent from the plan reader: " + selected
    (output / "selected-visit.txt").write_text(selected)
    terminal.dismiss()
    terminal.resize(60, 24)
    terminal.settle(0.3)
    narrow = terminal.screen.text()
    assert "Plan workflow" in narrow, "narrow graph title missing: " + narrow
    (output / "graph-narrow.txt").write_text(narrow)
    terminal.write(b"\x1b[B" * 5)
    terminal.settle(0.2)
    (output / "graph-narrow-scrolled.txt").write_text(terminal.screen.text())
    terminal.write(b"s")
    terminal.wait_text("Select plan agent provider/model", 5)
    (output / "send-provider-picker.txt").write_text(terminal.screen.text())
    terminal.dismiss()
    terminal.write(b"\t")
    terminal.settle(0.2)
    assert "Enter draft handoff" in terminal.screen.text(), "previous participant list is absent"
    (output / "previous-participants.txt").write_text(terminal.screen.text())
    terminal.write(b"\t")
    terminal.settle(0.2)
    terminal.dismiss()
    manual_start = sum(entry["lane"] == "b" for entry in provider.requests)
    parent_start = sum(entry["lane"] == "orchestrator" for entry in provider.requests)
    terminal.command("/plan send review b MANUAL_B_REVIEW")

    def manual_saved():
        assert not provider.errors, provider.errors
        record = terminal.saved()
        if not record:
            return None
        current = record[1].get("work_state", {}).get("plan_workflow")
        return current if current and len(current["steps"]) == 6 and current["steps"][-1]["status"] == "completed" else None

    manual = wait(manual_saved, terminal, timeout=20)
    assert manual["steps"][-1]["participant"] == "b" and manual["steps"][-1]["feedback"] == FEEDBACK, manual
    assert terminal.saved()[1]["work_state"]["plan"] == work["plan"], "manual review replaced the committed plan"
    assert sum(entry["lane"] == "b" for entry in provider.requests) == manual_start + 2, "manual review did not reuse B's route"
    assert sum(entry["lane"] == "orchestrator" for entry in provider.requests) == parent_start, "manual handoff unnecessarily invoked the parent provider"
    (output / "manual-handoff.json").write_text(json.dumps(manual, indent=2) + "\n")
    terminal.write(b"\x1b[F")
    terminal.settle(0.2)
    (output / "manual-handoff-graph.txt").write_text(terminal.screen.text())
    terminal.write(b"\r")
    terminal.settle(0.2)
    terminal.write(b"f")
    terminal.wait_text(FEEDBACK, 5)
    (output / "manual-feedback-reader.txt").write_text(terminal.screen.text())
    terminal.dismiss()
    terminal.dismiss()
    assert digest(terminal.config) == terminal.config_sha256, "fixture configuration changed"
    return {"saved_session": str(path), "session_id": terminal.session_id, "planning_visits": provider.plan_stages,
            "proof": str(proof), "proof_sha256": digest(proof), "markdown": [str(file) for file in markdown],
            "immutable_revisions": [str(file) for file in sorted(revisions)],
            "manual_handoff": {"participant": "b", "route_reused": True, "plan_preserved": True, "parent_request_needed": False},
            "fixture_config_unchanged": True, "routes": dict(MODEL)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    output = args.output.absolute()
    output.mkdir(parents=True, exist_ok=False)
    parent = "/private/var/tmp" if Path("/private/var/tmp").exists() else "/var/tmp"
    root = Path(tempfile.mkdtemp(prefix="ca-plan-workflow-", dir=parent)).resolve()
    receipt = {"schema": "ca.plan-workflow-pty.v1", "status": "running", "root": str(root),
        "binary": str(binary), "binary_sha256": digest(binary), "started": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    receipt["harness_sha256"] = digest(Path(__file__))
    provider, terminal = Provider(), None

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
        receipt["fixture_errors"] = provider.errors
        receipt["requests"] = {lane: sum(entry["lane"] == lane for entry in provider.requests) for lane in MODEL}
        receipt["binary_unchanged"] = receipt["binary_sha256"] == digest(binary)
        if not receipt["binary_unchanged"]:
            receipt["status"] = "failed"
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, indent=2))
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
