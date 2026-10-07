#!/usr/bin/env python3
"""Disposable no-provider PTY responsiveness/idle soak; standard library only.

Run: python3 tools/bug257_pty.py --binary target/release/ca --soak-seconds 28800
The child alone uses a temporary runtime root. No live config/session is read.
Reports keyboard-to-visible-frame latency, ordered paste, resize/theme safety,
and alternate-screen restoration. An eight-hour result requires eight hours.
"""
import argparse
import codecs
import datetime
import fcntl
import hashlib
import json
import math
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
import time
import unicodedata


class Screen:
    """Small VT reader for the absolute-position/erase sequences used by ratatui."""
    def __init__(self, width=100, height=30):
        self.resize(width, height)
        self.pending = ""
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")

    def resize(self, width, height):
        self.width, self.height = width, height
        self.cells = [[" " for _ in range(width)] for _ in range(height)]
        self.x = self.y = 0

    def feed(self, data):
        self.pending += self.decoder.decode(data)
        index = 0
        while index < len(self.pending):
            char = self.pending[index]
            if char == "\x1b":
                if index + 1 >= len(self.pending):
                    break
                if self.pending[index + 1] == "[":
                    match = re.match(r"\x1b\[([0-?]*)([ -/]*)([@-~])", self.pending[index:])
                    if not match:
                        break
                    raw, _, command = match.groups()
                    params = [int(value or "0") for value in raw.split(";")] if not raw.startswith(("?", ">", "<", "=")) else []
                    if command in "Hf" and params:
                        self.y = min(self.height - 1, max(0, (params[0] or 1) - 1))
                        self.x = min(self.width - 1, max(0, (params[1] if len(params) > 1 else 1) - 1))
                    elif command == "G" and params:
                        self.x = min(self.width - 1, max(0, (params[0] or 1) - 1))
                    elif command in "ABCD" and params:
                        amount = params[0] or 1
                        self.x = min(self.width - 1, max(0, self.x + (amount if command == "C" else -amount if command == "D" else 0)))
                        self.y = min(self.height - 1, max(0, self.y + (amount if command == "B" else -amount if command == "A" else 0)))
                    elif command == "J" and params and params[0] in (2, 3):
                        self.cells = [[" " for _ in range(self.width)] for _ in range(self.height)]
                    elif command == "K":
                        mode = params[0] if params else 0
                        start, end = (0, self.width) if mode == 2 else (0, self.x + 1) if mode == 1 else (self.x, self.width)
                        self.cells[self.y][start:end] = [" "] * (end - start)
                    index += len(match.group())
                    continue
                if self.pending[index + 1] == "]":
                    match = re.search(r"\x07|\x1b\\", self.pending[index + 2:])
                    if not match:
                        break
                    index += 2 + match.end()
                    continue
                index += 2
                continue
            if char == "\r":
                self.x = 0
            elif char == "\n":
                self.y = min(self.height - 1, self.y + 1)
            elif char >= " " and char != "\x7f":
                if unicodedata.combining(char):
                    if self.x:
                        self.cells[self.y][self.x - 1] += char
                else:
                    if self.x >= self.width:
                        self.x = 0
                        self.y = min(self.height - 1, self.y + 1)
                    width = 2 if unicodedata.east_asian_width(char) in ("W", "F") else 1
                    self.cells[self.y][self.x] = char
                    if width == 2 and self.x + 1 < self.width:
                        self.cells[self.y][self.x + 1] = ""
                    self.x += width
            index += 1
        self.pending = self.pending[index:]
        assert len(self.pending) < 65536, "unterminated terminal sequence"

    def text(self):
        return "\n".join("".join(row) for row in self.cells)


class Fixture:
    def __init__(self, binary, root):
        self.screen = Screen()
        self.bytes = 0
        self.raw_tail = b""
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))

        def terminal_session():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        # Process-local isolation; the parent environment and real runtime root are unchanged.
        environment = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(root),
                       "TERM": "xterm-256color", "LANG": "en_US.UTF-8", "COLORFGBG": "15;0"}
        self.process = subprocess.Popen([str(binary), "--profile", "bare", "--no-session",
                                        "--exclude-workspace-context", "--conversation-history-file", str(root / "history.json")],
                                       cwd=root, stdin=slave, stdout=slave, stderr=slave,
                                       env=environment, preexec_fn=terminal_session)
        os.close(slave)

    def read(self, timeout=0.05):
        if select.select([self.master], [], [], timeout)[0]:
            try:
                data = os.read(self.master, 65536)
            except OSError:
                data = b""
            if data:
                self.bytes += len(data)
                self.raw_tail = (self.raw_tail + data)[-65536:]
                assert b"\x1b]11;?" not in self.raw_tail, "unsafe OSC11 query emitted"
                self.screen.feed(data)
        if self.process.poll() is not None:
            return False
        return True

    def wait_text(self, value, timeout=5):
        deadline = time.monotonic() + timeout
        while value not in self.screen.text():
            assert self.read(), f"ca exited {self.process.returncode}: {self.screen.text()}"
            assert time.monotonic() < deadline, f"text {value!r} did not appear: {self.screen.text()}"

    def write(self, value):
        os.write(self.master, value)

    def resize(self, width, height):
        self.screen.resize(width, height)
        fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
        os.kill(self.process.pid, signal.SIGWINCH)

    def latency(self, count=32):
        # The previous draft is cleared using real backspace input, never submitted.
        self.write(b"\x7f" * 100)
        for _ in range(8):
            self.read(0.05)
        prefix, samples = "", []
        for index in range(count):
            char = chr(ord("a") + index % 26)
            prefix += char
            start = time.monotonic()
            self.write(char.encode())
            self.wait_text(prefix, 2)
            samples.append((time.monotonic() - start) * 1000)
        samples.sort()
        p95 = samples[min(len(samples) - 1, math.ceil(len(samples) * 0.95) - 1)]
        assert p95 < 100, f"keyboard-to-visible-frame p95 {p95:.2f}ms"
        return {"samples": count, "p95_ms": round(p95, 3), "max_ms": round(max(samples), 3)}

    def close(self):
        if self.process.poll() is None:
            self.write(b"\x03")
            deadline = time.monotonic() + 5
            while self.process.poll() is None and time.monotonic() < deadline:
                self.read(0.05)
        if self.process.poll() is None:
            self.process.terminate()
        self.process.wait(timeout=5)
        os.close(self.master)


def prepare(root):
    config = root / ".config/ca"
    config.mkdir(parents=True)
    (config / "config.toml").write_text("trusted_workspaces = [" + json.dumps(str(root)) + "]\n"
        "[providers.fixture]\nbase_url = 'http://127.0.0.1:1/v1'\napi_key = ''\nmodels = ['fixture-model']\n"
        "[defaults]\nprovider = 'fixture'\nmodel = 'fixture-model'\n"
        "[harness_improvement]\nenabled = false\n")
    stamp = "2026-01-01T00:00:00Z"
    messages = []
    for index in range(2048):
        messages.append({"role": "user", "content": f"Synthetic request {index}", "thinking": None, "timestamp": stamp, "tool_calls": None})
        messages.append({"role": "assistant", "content": f"Synthetic **reply** {index} 你好 with `code`.", "thinking": None, "timestamp": stamp,
                         "tool_calls": [{"id": f"fixture-{index}-{call}", "name": "read_file", "arguments": '{"path":"fixture.txt"}',
                                         "output": "synthetic fixture result", "status": "done"} for call in range(2)]})
    (root / "history.json").write_text(json.dumps({"schema": "ca.session-export.v1", "title": "BUG-257 disposable fixture",
        "created_at": stamp, "source_provider": "fixture", "source_model": "fixture-model", "messages": messages}))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--soak-seconds", type=float, default=0)
    parser.add_argument("--receipt", type=Path, help="Known output path for background release gates")
    args = parser.parse_args()
    if not math.isfinite(args.soak_seconds) or args.soak_seconds < 0:
        parser.error("--soak-seconds must be finite and nonnegative")
    root = Path(tempfile.mkdtemp(prefix="ca-bug257-pty-", dir="/private/tmp")).resolve()
    binary = args.binary.resolve(strict=True)
    binary_digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    receipt = args.receipt.resolve() if args.receipt else root / "result.json"
    result = {"status": "running", "root": str(root), "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "binary": str(binary), "binary_sha256": binary_digest, "harness_pid": os.getpid(),
              "soak_seconds_requested": args.soak_seconds, "messages": 4096, "phase": "startup"}
    prepare(root)
    fixture = Fixture(binary, root)
    result["child_pid"] = fixture.process.pid

    def report(emit=True):
        result["last_checked_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        for target in {root / "result.json", receipt}:
            temporary = target.with_name(target.name + ".pending")
            with temporary.open("w") as output:
                os.chmod(temporary, 0o600)
                output.write(json.dumps(result, indent=2) + "\n")
                output.flush()
                os.fsync(output.fileno())
            os.replace(temporary, target)
        if emit:
            print(json.dumps(result), flush=True)

    try:
        fixture.wait_text("fixture-model", 20)
        fixture.wait_text("Synthetic", 20)
        result["initial_latency"] = fixture.latency()
        result["ready_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        result["phase"] = "idle"
        report()
        started = time.monotonic()
        refreshed = started
        # No keys, paste, resize or focus during the idle interval.
        while time.monotonic() - started < args.soak_seconds:
            assert fixture.read(1), "ca exited during idle soak"
            if time.monotonic() - refreshed >= 60:
                result["idle_seconds_observed"] = round(time.monotonic() - started, 3)
                rss = subprocess.run(["/bin/ps", "-o", "rss=", "-p", str(fixture.process.pid)],
                                     capture_output=True, text=True, timeout=2, check=False)
                if rss.returncode == 0 and rss.stdout.strip().isdigit():
                    result["peak_sampled_rss_kib"] = max(result.get("peak_sampled_rss_kib", 0), int(rss.stdout))
                report(emit=False)
                refreshed = time.monotonic()
        result["idle_seconds_observed"] = round(time.monotonic() - started, 3)
        result["phase"] = "post_idle_checks"
        result["after_idle_latency"] = fixture.latency()
        fixture.resize(60, 24)
        fixture.wait_text("fixture-model")
        result["narrow_latency"] = fixture.latency()
        fixture.resize(100, 30)
        fixture.wait_text("fixture-model")
        fixture.write(b"\x7f" * 100 + b"/theme sync\r")
        fixture.wait_text("background detected")
        fixture.write(b"\x1b[200~ordered-paste-257\x1b[201~")
        fixture.wait_text("ordered-paste-257")
        result["ordered_paste_and_theme"] = True
        fixture.close()
        result["exit_code"] = fixture.process.returncode
        result["alternate_screen_restored"] = b"\x1b[?1049l" in fixture.raw_tail
        assert result["exit_code"] == 0 and result["alternate_screen_restored"]
        result["output_bytes"] = fixture.bytes
        assert hashlib.sha256(binary.read_bytes()).hexdigest() == binary_digest, "candidate changed during validation"
        result["status"] = "passed"
        result["phase"] = "complete"
    except BaseException as error:
        result["status"] = "failed"
        result["error"] = str(error)
        result["screen"] = fixture.screen.text()
        if fixture.process.poll() is None:
            fixture.close()
        raise
    finally:
        report()


if __name__ == "__main__":
    main()
