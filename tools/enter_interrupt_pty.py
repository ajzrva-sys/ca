#!/usr/bin/env python3
"""Reproduce Enter during a foreground command with the screenshot's draft.

Uses a loopback fake provider and disposable profiles, workspaces and sessions.
No user configuration, credentials, installed executable or live session changes.
Run: python3 tools/enter_interrupt_pty.py --binary target/release/ca --receipt /tmp/enter.json
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import shlex
import sys
import tempfile
import time

from orc258_pty import FakeProvider, Walkthrough, wait


DRAFT = "/Volumes/projects/qui you can use this source"


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def exercise(binary, root, provider, width, height, draft=DRAFT, queue_first=False,
             empty_completion=False):
    root.mkdir(mode=0o700)
    terminal = Walkthrough(binary, root, provider, name="Enter interrupt fixture")
    marker = "ENTER_INTERRUPT_WORK_" + root.name
    reply = "ENTER_INTERRUPT_REPLACEMENT_" + root.name
    sleeper = terminal.workspace / "foreground.py"
    started = terminal.workspace / "foreground.started"
    heartbeat = terminal.workspace / "foreground.heartbeat"
    finished = terminal.workspace / "foreground.finished"
    sleeper.write_text("import os, time\nfrom pathlib import Path\n"
        "Path('foreground.started').write_text(str(os.getpid()))\n"
        "deadline = time.monotonic() + 45\n"
        "while time.monotonic() < deadline:\n"
        "    Path('foreground.heartbeat').write_text(str(time.monotonic_ns()))\n"
        "    time.sleep(0.05)\n"
        "Path('foreground.finished').write_text('foreground completed')\n")
    provider.arm(marker, calls=[("foreground-" + str(width), "run_command", {
        "command": shlex.quote(sys.executable) + " " + shlex.quote(str(sleeper)),
        "background": False,
    })])
    provider.arm(draft, text=reply)
    case = {"width": width, "height": height, "draft": draft, "queue_first": queue_first,
            "empty_completion": empty_completion,
            "status": "running"}
    try:
        terminal.resize(width, height)
        terminal.settle()
        terminal.command(marker)
        wait(started.exists, terminal, timeout=10)
        terminal.wait_text("Executing run_command", 10)
        terminal.wait_text("Enter interrupt", 5)
        pid = int(started.read_text())
        case["foreground_pid"] = pid
        initial_heartbeat = heartbeat.read_text()
        wait(lambda: heartbeat.read_text() != initial_heartbeat, terminal, timeout=2)
        assert alive(pid), "foreground command exited before the Enter scenario"
        # Bracketed paste leaves the nonempty draft ready for a real CR key.
        pasted = draft + (" " if empty_completion else "")
        terminal.write(b"\x1b[200~" + pasted.encode() + b"\x1b[201~")
        if empty_completion:
            # A normal edit refreshes completion after paste, leaving the
            # absent absolute path as the active token under an empty menu.
            terminal.write(b"\x7f")
            terminal.wait_text("No path matches", 5)
            terminal.wait_text("Paths 0/0", 5)
            case["empty_path_menu_observed"] = True
        terminal.wait_text(draft, 5)
        terminal.settle()
        if queue_first:
            before_queue = len(provider.requests)
            terminal.write(b"\x1b\r")
            terminal.settle(0.3)
            assert len(provider.requests) == before_queue, "Alt+Enter sent instead of queueing"
            assert alive(pid), "Alt+Enter interrupted the command"
            assert "Queued · " in terminal.screen.text(), terminal.screen.text()
            case["alt_enter_queued_without_interrupt"] = True
            terminal.write(b"\x1b[200~" + draft.encode() + b"\x1b[201~")
            terminal.settle()
        case["before_enter_screen"] = terminal.screen.text()
        previous = len(provider.requests)
        start = time.monotonic()
        terminal.write(b"\r")
        wait(lambda: len(provider.requests) > previous, terminal, timeout=5)
        replacement = provider.requests[-1]
        assert replacement["user"] == draft, replacement
        assert replacement["last_role"] == "user", replacement
        terminal.wait_text(reply, 5)
        case["enter_to_replacement_ms"] = round((time.monotonic() - start) * 1000, 3)
        wait(lambda: not alive(pid), terminal, timeout=5)
        assert not finished.exists(), "old foreground command ran to completion"
        stable_heartbeat = heartbeat.read_text()
        terminal.settle(0.3)
        assert heartbeat.read_text() == stable_heartbeat, "old foreground command still runs"
        assert len(provider.requests) == previous + 1, provider.requests[previous:]
        assert "Executing run_command" not in terminal.screen.text(), terminal.screen.text()
        case["foreground_cancelled"] = True
        case["after_enter_screen"] = terminal.screen.text()
        case["status"] = "passed"
    except Exception as error:
        case["status"] = "failed"
        case["error"] = repr(error)
        case["terminal"] = terminal.screen.text()
    finally:
        if terminal.process.poll() is None:
            # A failed interrupt leaves the draft and foreground turn active.
            # Explicitly stop, clear the draft, and then exit the owned terminal.
            for _ in range(2):
                terminal.write(b"\x1b")
                terminal.settle(0.2)
            terminal.write(b"\x7f" * 200)
            terminal.settle(0.2)
        terminal.close()
        case["terminal_exit"] = terminal.process.returncode
        case["terminal_restored"] = b"\x1b[?1049l" in terminal.raw_tail
        if case["terminal_exit"] != 0 or not case["terminal_restored"]:
            case["status"] = "failed"
            case["cleanup_error"] = ("terminal exit " + str(case["terminal_exit"])
                + "; alternate screen restored: " + str(case["terminal_restored"]))
    return case


def exercise_idle(binary, root, provider):
    root.mkdir(mode=0o700)
    terminal = Walkthrough(binary, root, provider, name="Idle absolute path fixture")
    provider.arm(DRAFT, text="ENTER_IDLE_PATH_RECEIVED")
    case = {"scenario": "idle", "draft": DRAFT, "status": "running"}
    try:
        previous = len(provider.requests)
        terminal.command(DRAFT)
        terminal.wait_text("ENTER_IDLE_PATH_RECEIVED", 5)
        assert len(provider.requests) == previous + 1, provider.requests[previous:]
        assert provider.requests[-1]["user"] == DRAFT, provider.requests[-1]
        case["status"] = "passed"
    except Exception as error:
        case["status"], case["error"] = "failed", repr(error)
        case["terminal"] = terminal.screen.text()
    finally:
        if terminal.process.poll() is None:
            terminal.write(b"\x7f" * 200)
            terminal.settle(0.2)
        terminal.close()
        case["terminal_exit"] = terminal.process.returncode
        case["terminal_restored"] = b"\x1b[?1049l" in terminal.raw_tail
        if case["terminal_exit"] != 0 or not case["terminal_restored"]:
            case["status"] = "failed"
            case["cleanup_error"] = ("terminal exit " + str(case["terminal_exit"])
                + "; alternate screen restored: " + str(case["terminal_restored"]))
    return case


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix="ca-enter-interrupt-pty-", dir="/private/tmp")).resolve()
    os.chmod(root, 0o700)
    result = {"schema": "ca.enter-interrupt-pty-receipt.v1", "status": "running",
              "root": str(root), "binary": str(binary),
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(), "cases": []}
    provider = None
    try:
        provider = FakeProvider()
        no_match = "/Volumes/projects/qui-enter-fixture-" + root.name.rsplit("-", 1)[-1]
        assert not Path(no_match).exists(), "path-only fixture unexpectedly exists"
        for label, width, height, draft, queue_first, empty_completion in (
            ("screenshot-wide", 100, 30, DRAFT, False, False),
            ("screenshot-narrow", 60, 24, DRAFT, False, False),
            ("path-only-queue", 100, 30, no_match, True, False),
            ("path-only-empty-menu", 100, 30, no_match, False, True),
        ):
            case = exercise(binary, root / label, provider, width, height, draft,
                            queue_first, empty_completion)
            case["scenario"] = label
            result["cases"].append(case)
            if case["status"] != "passed":
                break
        if len(result["cases"]) == 4 and all(case["status"] == "passed" for case in result["cases"]):
            result["cases"].append(exercise_idle(binary, root / "idle", provider))
        result["status"] = "passed" if len(result["cases"]) == 5 and all(
            case["status"] == "passed" for case in result["cases"]) else "failed"
    except Exception as error:
        result["status"], result["error"] = "failed", repr(error)
    finally:
        if provider:
            provider.close()
        result["requests"] = provider.requests if provider else []
        result["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(json.dumps(result, indent=2) + "\n")
        os.chmod(args.receipt, 0o600)
        print(json.dumps({"status": result["status"], "root": str(root), "error": result.get("error"),
                          "cases": [{key: case.get(key) for key in (
                              "scenario", "width", "height", "status", "enter_to_replacement_ms", "error")}
                              for case in result["cases"]], "receipt": str(args.receipt)}), flush=True)
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
