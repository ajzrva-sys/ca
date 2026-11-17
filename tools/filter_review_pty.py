#!/usr/bin/env python3
"""Ordinary editable filter review and automatic execution, in disposable state.

python3 tools/filter_review_pty.py --binary PATH --output NEW_DIRECTORY
No real provider, credentials, production profile, or installed-state writes.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import tempfile
import threading
import time

from plan_workflow_pty import Provider, Terminal, functions, message_text, tool_result
from orc258_pty import wait

TITLE = "Enter  Send and run"
INSPECTOR = "Filter on · this task"
PRIVATE_HOST = "private-fixture.example.test"
SAVED_PRIVATE = "SAVED_PRIVATE_CANARY"
CUSTOM_PRIVATE = "CUSTOM_PRIVATE_VALUE"
EVIDENCE = f"REPOSITORY_EVIDENCE {SAVED_PRIVATE}\n"


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def request_alias(text):
    # The fixture consumes only references actually disclosed by the application.
    matches = re.findall(r"__CA_PATH_[A-Za-z0-9_]+__/(?:d[A-Za-z0-9_]+/)*f[A-Za-z0-9_]+(?:\.[A-Za-z0-9]+)?", text)
    assert matches, "planner did not receive the attached file's executable reference"
    return matches[0]


def proposal(number, reference):
    title = f"Inspect task {number}"
    return {
        "title": f"Filter review task {number}",
        "body": ("## Shared purpose and constraints\nInspect the supplied evidence locally.\n"
                 f"## Task: {title}\nRead {reference} and report progress.\n"),
        "tasks": [title],
        "graph": {
            "budget": {"allow_unpriced": True, "max_physical_requests": 30},
            "tasks": [{"task_id": title, "tools": ["read_file"], "paths": [reference],
                       "commands": [], "evidence": [{"kind": "artifact", "path": reference,
                           "artifact_kind": "regular_file", "min_bytes": 1}]}],
        },
    }


class ReviewProvider(Provider):
    def __init__(self):
        self.second = threading.Event()
        self.finish = threading.Event()
        self.authors = 0
        self.references = {}
        self.worker_requests = {1: 0, 2: 0}
        super().__init__()

    def hold(self, event):
        self.lock.release()
        try:
            assert event.wait(55), "fixture pending-response hold timed out"
        finally:
            self.lock.acquire()

    def response(self, lane, payload):
        assert lane == "orchestrator", lane
        names, text = functions(payload), message_text(payload)
        assert PRIVATE_HOST not in text, "private URL host reached provider"
        assert SAVED_PRIVATE not in text, "saved explicit hiding rule was lost"
        assert "evidence.txt" not in text, "private source filename reached provider"
        if "report_structured_output" in names:
            assert not {"write_file", "edit_file", "run_command", "spawn_agent"}.intersection(names)
            self.authors += 1
            number = self.authors
            assert number in (1, 2), "unexpected duplicate planner or correction request"
            if number == 1:
                assert "EPT" in text, "default preset silently selected technical-word suggestions"
                assert CUSTOM_PRIVATE in text, "default preset changed an unselected custom word"
            else:
                assert not re.search(r"\bEPT\b", text), "selected identifier suggestion reached provider"
                assert CUSTOM_PRIVATE not in text, "edited custom suggestion reached provider"
            assert "REPOSITORY_EVIDENCE" in text, "attachment content was lost"
            reference = request_alias(text)
            self.references[number] = reference
            return [(f"plan-{number}", "report_structured_output", {"value": proposal(number, reference)})], ""
        number = next((number for number in self.references if f"## Task: Inspect task {number}" in text), None)
        assert number is not None, "worker did not receive its scoped task"
        self.worker_requests[number] += 1
        reference = self.references[number]
        first_id, second_id = f"read-{number}-first", f"read-{number}-second"
        if tool_result(payload, first_id) is None:
            return [(first_id, "read_file", {"path": reference})], f"TASK-{number}-FIRST-PENDING"
        assert "REPOSITORY_EVIDENCE" in str(tool_result(payload, first_id)), "native read did not restore the source reference"
        if number == 1 and tool_result(payload, second_id) is None:
            self.hold(self.second)
            return [(second_id, "read_file", {"path": reference})], "TASK-1-SECOND-PENDING"
        if number == 1:
            self.hold(self.finish)
        return [], f"TASK-{number}-COMPLETE"


def seed_start(original):
    def launch(args, **kwargs):
        args = list(args)
        if "--new-session-id" in args:
            index = args.index("--new-session-id")
            session_id = args[index + 1]
            args[index] = "--session"
            runtime, workspace = Path(kwargs["env"]["HOME"]), Path(kwargs["cwd"])
            directory = runtime / ".local/share/ca/sessions"
            directory.mkdir(parents=True, exist_ok=True)
            stamp = "2026-11-09T12:00:00Z"
            messages = [{"role": "User" if n % 2 == 0 else "Assistant",
                         "content": f"History {n}: representative retained previous work with a long explanatory sentence.",
                         "tool_calls": None, "thinking": None, "timestamp": stamp} for n in range(192)]
            (directory / (session_id + ".json")).write_text(json.dumps({
                "version": 2, "id": session_id, "title": "Filter review history fixture",
                "workspace": str(workspace), "created_at": stamp, "updated_at": stamp,
                "provider": "orchestrator", "model": "workflow-orchestrator", "parent": None,
                "messages": messages, "context_checkpoint": None}))
            config = runtime / ".config/ca/config.toml"
            config.write_text(config.read_text() +
                "\n[agent]\nreview_changes=false\nproactive_delegation=false\n"
                "\n[ui]\nfilter_preview=true\n"
                "\n[provider_filter]\nenabled=true\n[provider_filter.hide]\nwords=['" + SAVED_PRIVATE + "']\n")
            (workspace / "evidence.txt").write_text(EVIDENCE)
        return original(args, **kwargs)
    return launch


def ready(terminal, title=TITLE):
    terminal.wait_text(title, 20)
    wait(lambda: title in terminal.screen.text()
         and "Preparing the provider copy locally" not in terminal.screen.text()
         and "Updating preview" not in terminal.screen.text(), terminal, timeout=20)
    terminal.settle(0.15)


def capture(terminal, output, name):
    terminal.settle(0.15)
    text = terminal.screen.text()
    (output / (name + ".txt")).write_text(text)
    return text


def check_requests(provider, count):
    assert not provider.errors, provider.errors
    assert len(provider.requests) == count, f"expected {count} provider requests, got {len(provider.requests)}"


def completed(terminal, provider, task=None):
    assert not provider.errors, provider.errors
    record = terminal.saved()
    if not record:
        return None
    work = record[1].get("work_state", {})
    if task is not None:
        plan = work.get("plan") or {}
        graph = work.get("agent_graph", {})
        if (plan.get("title") != f"Filter review task {task}"
                or graph.get("plan_id") != plan.get("id")):
            return None
    assert work.get("agent_graph", {}).get("status") not in ("failed", "cancelled"), work
    return work if work.get("agent_graph", {}).get("status") == "completed" else None


def scenario(terminal, provider, output):
    configuration_hash = sha256(terminal.config)
    evidence_hash = sha256(terminal.workspace / "evidence.txt")
    first_task = f"FIRST_TASK: Inspect @evidence.txt for EPT and {CUSTOM_PRIVATE}. The reference URL is https://{PRIVATE_HOST}/example."
    terminal.command(first_task)
    ready(terminal)
    check_requests(provider, 0)
    screen = capture(terminal, output, "default-preview-wide")
    assert "Private values + suggestions" not in screen, "suggestions were selected by default"
    assert "Private values" in screen and "Send and run" in screen
    terminal.write(b"2")
    terminal.wait_text(CUSTOM_PRIVATE, 5)
    capture(terminal, output, "original-wide")
    terminal.write(b"3")
    ready(terminal)
    capture(terminal, output, "provider-copy-wide")
    terminal.resize(70, 30)
    ready(terminal)
    capture(terminal, output, "default-preview-narrow")
    terminal.write(b"\x1b")
    terminal.wait_text("FIRST_TASK", 5)
    check_requests(provider, 0)
    capture(terminal, output, "cancelled-draft")
    terminal.write(b"\r")
    ready(terminal)
    check_requests(provider, 0)
    terminal.resize(120, 36)
    ready(terminal)
    terminal.write(b"\r")
    terminal.wait_text("TASK-1-FIRST-PENDING", 45)
    assert provider.authors == 1, "Send did not make exactly one planner submission"
    work = terminal.saved()[1]["work_state"]
    assert work["plan"]["approved_by"] == "automatic_task_send", "automatic execution was recorded as manual plan review"
    assert work["agent_graph"]["status"] == "running", "graph waited for a second plan approval"
    capture(terminal, output, "first-task-map-pending")
    terminal.command("/filter")
    ready(terminal, INSPECTOR)
    terminal.write(b"3")
    terminal.wait_text("TASK-1-FIRST-PENDING", 15)
    capture(terminal, output, "first-filter-pending")
    provider.second.set()
    terminal.wait_text("TASK-1-SECOND-PENDING", 25)
    second = capture(terminal, output, "second-filter-pending-wide")
    assert second.find("TASK-1-SECOND-PENDING") >= 0
    if "TASK-1-FIRST-PENDING" in second:
        assert second.find("TASK-1-SECOND-PENDING") < second.find("TASK-1-FIRST-PENDING"), "old content still leads the pending preview"
    terminal.resize(70, 30)
    terminal.wait_text("TASK-1-SECOND-PENDING", 10)
    capture(terminal, output, "second-filter-pending-narrow")
    terminal.write(b"\x1b")
    provider.finish.set()
    wait(lambda: completed(terminal, provider), terminal, timeout=35)
    terminal.resize(120, 36)
    second_task = f"SECOND_TASK: Inspect @evidence.txt for EPT and {CUSTOM_PRIVATE} using https://{PRIVATE_HOST}/other."
    before = len(provider.requests)
    terminal.command(second_task)
    ready(terminal)
    assert "Private values + suggestions" not in terminal.screen.text(), "new task inherited the previous preset"
    check_requests(provider, before)
    terminal.write(b"p")
    ready(terminal)
    terminal.wait_text("Private values + suggestions", 5)
    terminal.write(b"a")
    terminal.wait_text("Term", 5)
    terminal.write(b"\x1b[200~EDITME\x1b[201~\r")
    ready(terminal)
    terminal.write(b"1e")
    terminal.wait_text("EDITME", 5)
    terminal.write(b"\x1b[H" + b"\x1b[3~" * 6 + b"\x1b[200~" + CUSTOM_PRIVATE.encode() + b"\x1b[201~\r")
    ready(terminal)
    terminal.write(b"r")
    terminal.wait_text("Remember for this project", 5)
    capture(terminal, output, "remember-option")
    terminal.write(b"r")
    terminal.write(b"4")
    ready(terminal)
    capture(terminal, output, "edited-suggestion-changes")
    check_requests(provider, before)
    terminal.write(b"3")
    ready(terminal)
    capture(terminal, output, "suggestions-provider-copy")
    terminal.write(b"\r")
    wait(lambda: provider.authors == 2 and completed(terminal, provider, task=2), terminal, timeout=45)
    assert provider.worker_requests[2] >= 2, "second task did not execute native read and continuation"
    terminal.save()
    path, record = terminal.saved()
    envelope = json.loads(path.read_text())
    assert envelope.get("message_count", len(record.get("messages", []))) >= 194, "representative history was lost"
    assert sha256(terminal.config) == configuration_hash, "task edits changed saved profile settings"
    assert sha256(terminal.workspace / "evidence.txt") == evidence_hash, "filter workflow altered source bytes"
    capture(terminal, output, "completed")
    (output / "saved-work.json").write_text(json.dumps(record["work_state"], indent=2))
    return {"requests": len(provider.requests), "planners": provider.authors,
            "history_messages": envelope.get("message_count", len(record.get("messages", []))),
            "no_provider_before_send": True, "cancellation_preserved_draft": True,
            "two_selectable_modes": True, "edited_suggestion_sent": True,
            "no_second_plan_confirmation": True, "two_live_pending_preview_updates": True,
            "source_unchanged": True, "saved_profile_unchanged": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    output = args.output.absolute()
    output.mkdir(parents=True, exist_ok=False)
    parent = "/private/var/tmp" if Path("/private/var/tmp").exists() else "/var/tmp"
    root = Path(tempfile.mkdtemp(prefix="ca-filter-review-", dir=parent)).resolve()
    result = {"schema": "ca.filter-review-pty.v1", "status": "running", "root": str(root),
              "binary": str(binary), "binary_sha256": sha256(binary),
              "harness_sha256": sha256(Path(__file__)),
              "started": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    provider, terminal = ReviewProvider(), None
    original = subprocess.Popen
    def interrupted(signum, _frame):
        raise RuntimeError(f"Fixture interrupted by signal {signum}")
    for signum in (signal.SIGTERM, signal.SIGHUP):
        signal.signal(signum, interrupted)
    try:
        subprocess.Popen = seed_start(original)
        terminal = Terminal(binary, root, provider)
        subprocess.Popen = original
        result.update(scenario(terminal, provider, output), status="passed")
    except BaseException as error:
        result.update(status="failed", error=repr(error))
    finally:
        subprocess.Popen = original
        provider.second.set()
        provider.finish.set()
        if terminal:
            (output / "last-screen.txt").write_text(terminal.screen.text())
            terminal.close()
            (output / "terminal.raw").write_bytes(terminal.raw)
            result.update(exit_code=terminal.process.returncode, terminal_restored=terminal.restored_termios)
            if terminal.process.returncode != 0 or not terminal.restored_termios:
                result.update(status="failed", terminal_error="unclean exit or terminal restoration")
        provider.close()
        (output / "requests.json").write_text(json.dumps(provider.requests, indent=2))
        result["provider_errors"] = provider.errors
        (output / "receipt.json").write_text(json.dumps(result, indent=2))
    print(json.dumps(result))
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
