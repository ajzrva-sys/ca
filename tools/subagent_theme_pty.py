#!/usr/bin/env python3
"""Capture and verify a real delayed child in the charcoal conversation UI.

Uses only a disposable runtime/workspace and a loopback fixture provider.
Requires Pillow and pyte, like tui_screenshots.py. Outputs VT-derived PNGs,
request/lifecycle evidence and exact owned-terminal restoration evidence.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import tempfile
import termios

import orc258_pty
from tui_screenshots import CaptureScreen


def roster_seconds(terminal):
    for line in terminal.screen.text().splitlines():
        if "Explorer" in line and (match := re.search(r"\b(\d+)s\s*$", line)):
            return int(match.group(1))
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--font", default="/System/Library/Fonts/Menlo.ttc")
    args = parser.parse_args()
    binary = args.binary.resolve()
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    args.output.mkdir(parents=True, exist_ok=True)
    receipt = {"status": "running", "binary_sha256": digest,
        "provider": "Disposable loopback fixture; delayed real task child, no live model",
        "captures": []}
    provider = orc258_pty.FakeProvider()
    terminal = None
    retained_master = None
    original_termios = None
    try:
        with tempfile.TemporaryDirectory(prefix="ca-subagent-theme-", dir="/private/tmp") as directory:
            root = Path(directory).resolve()
            orc258_pty.Screen = CaptureScreen
            original_openpty = orc258_pty.pty.openpty

            def owned_openpty():
                nonlocal retained_master, original_termios
                master, slave = original_openpty()
                original_termios = termios.tcgetattr(slave)
                retained_master = os.dup(master)
                return master, slave

            orc258_pty.pty.openpty = owned_openpty
            try:
                terminal = orc258_pty.Walkthrough(binary, root, provider, name="Charcoal child UI fixture")
            finally:
                orc258_pty.pty.openpty = original_openpty
            terminal.resize(184, 40)
            terminal.settle(0.3)

            def capture(name):
                path = args.output / (name + ".png")
                terminal.screen.save(path, args.font)
                receipt["captures"].append({"path": path.name,
                    "columns": terminal.screen.width, "rows": terminal.screen.height,
                    "sha256": hashlib.sha256(path.read_bytes()).hexdigest()})

            capture("charcoal-idle-184")
            provider.arm("CHARCOAL_PARENT_UI", calls=[("theme-child", "task", {
                "subagent_name": "explore", "prompt": "CHARCOAL_CHILD_UI: inspect the release surfaces"})])
            provider.arm("CHARCOAL_CHILD_UI", text="CHARCOAL_CHILD_FINISHED")
            with provider.lock:
                provider.routes["CHARCOAL_CHILD_UI"]["delay"] = 12
            terminal.command("CHARCOAL_PARENT_UI", "Subagents 1", 10)
            terminal.wait_text("Explorer", 5)
            terminal.wait_text("Waiting for response", 5)
            first = orc258_pty.wait(lambda: roster_seconds(terminal), terminal, timeout=5)
            second = orc258_pty.wait(lambda: (value := roster_seconds(terminal)) is not None and value > first and value,
                terminal, timeout=5)
            receipt["active_timer_seconds"] = [first, second]
            assert second > first, "child timer did not advance without input"
            capture("charcoal-subagent-waiting-184")
            terminal.resize(60, 24)
            terminal.wait_text("Subagents 1", 5)
            terminal.wait_text("Explorer", 5)
            assert roster_seconds(terminal) is not None, "narrow roster lost its timer"
            capture("charcoal-subagent-waiting-60")
            terminal.resize(100, 30)
            terminal.wait_text("Subagents 1", 5)
            terminal.write(b"\x18\x1b[B")
            terminal.wait_text("select", 5)
            terminal.write(b"\r")
            terminal.wait_text("status:", 5)
            terminal.write(b"\x1b")
            terminal.settle(0.2)
            terminal.wait_text("Completed", 20)
            capture("charcoal-subagent-completed-100")
            frozen = roster_seconds(terminal)
            terminal.settle(1.2)
            assert roster_seconds(terminal) == frozen, "completed child duration changed"
            receipt["completed_timer_seconds"] = frozen
            _, saved = terminal.save()
            summaries = saved.get("transcript_agent_summaries", [])
            assert any(summary.get("role") == "Explorer" and summary.get("model") == "fixture-model"
                and summary.get("status") == "completed" for summary in summaries), "saved child metadata missing"
            receipt["saved_child_metadata"] = True
            with provider.lock:
                receipt["provider_requests"] = list(provider.requests)
            assert any("CHARCOAL_CHILD_UI" in request["user"] for request in receipt["provider_requests"]), "child made no request"
            closed_terminal = terminal
            terminal.close()
            terminal = None
            receipt["exit_code"] = closed_terminal.process.returncode
            receipt["alternate_screen_restored"] = b"\x1b[?1049l" in closed_terminal.raw_tail
            receipt["termios_restored"] = termios.tcgetattr(retained_master) == original_termios
            assert receipt["exit_code"] == 0 and receipt["termios_restored"] and receipt["alternate_screen_restored"]
            terminal = None
            assert hashlib.sha256(binary.read_bytes()).hexdigest() == digest, "candidate changed during capture"
            receipt["status"] = "passed"
    except BaseException as error:
        receipt["status"] = "failed"
        receipt["error"] = str(error)
        if terminal:
            receipt["failure_screen"] = terminal.screen.text()
        raise
    finally:
        if terminal:
            terminal.close()
        if retained_master is not None:
            os.close(retained_master)
        provider.close()
        (args.output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"status": receipt["status"], "captures": len(receipt["captures"]), "output": str(args.output)}))


if __name__ == "__main__":
    main()
