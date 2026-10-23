#!/usr/bin/env python3
"""Verify CA's package version stays visible through actual terminal use.

python3 tools/version_display_pty.py --binary target/release/ca --output scratch/version-display/release
Reuses the existing PTY reader, command interface and strict loopback provider.
All configuration and conversation state is disposable. Retains raw terminal
bytes, visible frames and exit/restoration checks without accessing live state.
"""
import argparse
import datetime
import fcntl
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
import time

from bug257_pty import Fixture, Screen
from compaction_pty import binary_digest
from orc258_pty import Walkthrough
from questionnaire_pty import Provider


PROMPT = "VERSION_DISPLAY_FIXTURE_PROMPT"
REPLY = "VERSION_DISPLAY_FIXTURE_COMPLETE"


def isolated_environment(runtime, root):
    return {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(runtime),
            "XDG_CONFIG_HOME": str(runtime / ".config"),
            "XDG_DATA_HOME": str(runtime / ".local/share"),
            "XDG_CACHE_HOME": str(runtime / ".cache"), "TMPDIR": str(root),
            "TERM": "xterm-256color", "LANG": "en_US.UTF-8", "COLORFGBG": "15;0"}


class Terminal(Walkthrough):
    """Existing terminal interactions with complete raw/restoration evidence."""
    def __init__(self, binary, root, provider, standalone=False):
        self.binary, self.root, self.provider = binary, root, provider
        self.standalone = standalone
        self.workspace, self.runtime = root / "workspace", root / "runtime"
        self.workspace.mkdir(mode=0o700)
        config = self.runtime / ".config/ca"
        config.mkdir(parents=True, mode=0o700)
        configuration = ("trusted_workspaces = [" + json.dumps(str(self.workspace)) + "]\n"
            "[providers.fixture]\nbase_url=" + json.dumps(provider.url) + "\n"
            "api_key=''\nmodels=['fixture-model']\nprotocol='chat_completions'\n"
            "[defaults]\nprovider='fixture'\nmodel='fixture-model'\n"
            "[harness_improvement]\nenabled=false\n"
            "[learning.dream]\nenabled=false\n[learning.distill]\nenabled=false\n")
        config_path = config / "config.toml"
        config_path.write_text(configuration)
        config_path.chmod(0o600)
        self.config_path = config_path
        self.environment = isolated_environment(self.runtime, root)
        self.screen = Screen(100, 36)
        self.bytes, self.raw_tail, self.raw = 0, b"", bytearray()
        self.closed, self.forced_exit, self.restored_termios = False, False, False
        self.master, slave = pty.openpty()
        self.original_termios = termios.tcgetattr(slave)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 100, 0, 0))

        def terminal_session():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        arguments = [str(binary), "personality"] if standalone else [str(binary),
            "--profile", "default", "--exclude-workspace-context", "--no-session",
            "--name", "Version display fixture"]
        try:
            self.process = subprocess.Popen(arguments, cwd=self.workspace, stdin=slave,
                stdout=slave, stderr=slave, env=self.environment, preexec_fn=terminal_session)
        except BaseException:
            os.close(self.master)
            raise
        finally:
            os.close(slave)

    def read(self, timeout=0.05):
        prior = self.bytes
        result = Fixture.read(self, timeout)
        if self.bytes > prior:
            self.raw.extend(self.raw_tail[-(self.bytes - prior):])
        assert len(self.raw) <= 16 * 1024 * 1024, "terminal output exceeded 16 MiB"
        return result

    def close(self):
        if self.closed:
            return
        if self.process.poll() is None:
            # Separate Escape events dismiss an editor or dashboard before exit.
            for _ in range(0 if self.standalone else 3):
                self.write(b"\x1b")
                self.read(0.1)
                if self.process.poll() is not None:
                    break
            if self.process.poll() is None:
                self.write(b"\x03")
            deadline = time.monotonic() + 5
            while self.process.poll() is None and time.monotonic() < deadline:
                self.read(0.03)
        if self.process.poll() is None:
            self.forced_exit = True
            os.killpg(self.process.pid, signal.SIGTERM)
        self.process.wait(timeout=5)
        self.read(0)
        self.restored_termios = termios.tcgetattr(self.master) == self.original_termios
        os.close(self.master)
        self.closed = True


def capture(terminal, label, expected, output, frames):
    terminal.settle(0.25)
    text = terminal.screen.text()
    path = output / (label + ".txt")
    path.write_text(text + "\n")
    rows = text.splitlines()
    frame = {"label": label, "columns": terminal.screen.width,
             "rows": terminal.screen.height, "raw_bytes_at_capture": terminal.bytes,
             "path": path.name, "sha256": binary_digest(path), "text": text,
             "expected_version_label": expected,
             "version_visible": any(expected in row for row in rows),
             "visible_label_rows": [index + 1 for index, row in enumerate(rows) if expected in row]}
    frames.append(frame)


def resize_capture(terminal, width, label, expected, output, frames):
    terminal.resize(width, 36)
    capture(terminal, label, expected, output, frames)


def exercise(terminal, expected, output, frames, standalone=False):
    if standalone:
        terminal.wait_text("Quick setup", 15)
        capture(terminal, "standalone-personality-100", expected, output, frames)
        resize_capture(terminal, 60, "standalone-personality-60", expected, output, frames)
        resize_capture(terminal, 24, "standalone-personality-24", expected, output, frames)
        terminal.wait_text("preserved.", 5)
        return
    terminal.wait_text("fixture-model", 15)
    capture(terminal, "startup-100", expected, output, frames)
    terminal.provider.arm(PROMPT, text=REPLY)
    terminal.command(PROMPT, REPLY, 15)
    capture(terminal, "completed-prompt-100", expected, output, frames)
    resize_capture(terminal, 60, "conversation-60", expected, output, frames)
    resize_capture(terminal, 24, "conversation-24", expected, output, frames)
    terminal.resize(100, 36)
    terminal.settle()
    terminal.command("/compact-mode")
    capture(terminal, "compact-100", expected, output, frames)
    assert re.search(r"(?m)^compact_mode\s*=\s*true\s*$", terminal.config_path.read_text()), \
        "compact mode was not enabled in disposable fixture configuration"
    resize_capture(terminal, 60, "compact-60", expected, output, frames)
    resize_capture(terminal, 24, "compact-24", expected, output, frames)
    terminal.resize(100, 36)
    terminal.settle()
    terminal.command("/compact-mode", "Compact mode disabled", 5)
    assert re.search(r"(?m)^compact_mode\s*=\s*false\s*$", terminal.config_path.read_text()), \
        "compact mode was not disabled in disposable fixture configuration"
    terminal.command("/new", "New session started", 10)
    capture(terminal, "new-session-100", expected, output, frames)
    assert PROMPT not in terminal.screen.text(), "new session retained the previous prompt"
    terminal.command("/dashboard", "+ New Agent", 10)
    capture(terminal, "dashboard-100", expected, output, frames)
    resize_capture(terminal, 60, "dashboard-60", expected, output, frames)
    resize_capture(terminal, 24, "dashboard-24", expected, output, frames)
    terminal.resize(100, 36)
    terminal.settle()
    terminal.dismiss()
    terminal.wait_text("fixture-model", 5)
    terminal.command("/personality", "Quick setup", 10)
    capture(terminal, "native-personality-100", expected, output, frames)
    resize_capture(terminal, 60, "native-personality-60", expected, output, frames)
    resize_capture(terminal, 24, "native-personality-24", expected, output, frames)
    terminal.wait_text("preserved.", 5)


def run_case(binary, root, provider, expected, output, standalone=False):
    root.mkdir(mode=0o700)
    terminal, frames = None, []
    result = {"status": "running", "frames": frames}
    try:
        terminal = Terminal(binary, root, provider, standalone)
        exercise(terminal, expected, output, frames, standalone)
        result["actions_completed"] = True
    except Exception as error:
        result["actions_completed"] = False
        result["error"] = str(error)
        if terminal is not None:
            result["failure_screen"] = terminal.screen.text()
    finally:
        if terminal is not None:
            try:
                terminal.close()
            except Exception as error:
                result["cleanup_error"] = str(error)
            raw_path = output / ("standalone-personality.ansi" if standalone else "interactive.ansi")
            raw_path.write_bytes(terminal.raw)
            raw = bytes(terminal.raw)
            entered = raw.rfind(b"\x1b[?1049h")
            result.update({"raw_path": raw_path.name, "output_bytes": terminal.bytes,
                "raw_sha256": binary_digest(raw_path),
                "exit_code": terminal.process.poll(), "forced_exit": terminal.forced_exit,
                "terminal_settings_restored": terminal.restored_termios,
                "alternate_screen_restored": entered >= 0 and raw.rfind(b"\x1b[?1049l") > entered,
                "cursor_restored": raw.rfind(b"\x1b[?25h") > max(entered, raw.rfind(b"\x1b[?25l")),
                "bracketed_paste_restored": raw.rfind(b"\x1b[?2004l") > raw.rfind(b"\x1b[?2004h") >= 0})
    result["missing_version_frames"] = [frame["label"] for frame in frames if not frame["version_visible"]]
    result["status"] = "passed" if (result.get("actions_completed") and frames
        and not result["missing_version_frames"] and result.get("exit_code") == 0
        and not result.get("forced_exit") and result.get("terminal_settings_restored")
        and result.get("alternate_screen_restored") and result.get("cursor_restored")
        and result.get("bracketed_paste_restored")
        and not result.get("cleanup_error")) else "failed"
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    manifest = (Path(__file__).resolve().parents[1] / "Cargo.toml").read_text()
    package = re.search(r"(?ms)^\[package\]\s*\n(.*?)(?=^\[|\Z)", manifest)
    version = re.search(r'(?m)^version\s*=\s*"([^"]+)"\s*$', package.group(1)) if package else None
    assert version, "Cargo.toml lacks a package version"
    package_version, expected = version.group(1), "ca v" + version.group(1)
    receipt = {"schema": "ca.version-display-pty-receipt.v1", "status": "running",
        "binary": str(binary), "binary_sha256": binary_digest(binary),
        "probe_sha256": binary_digest(Path(__file__).resolve()),
        "package_version": package_version, "expected_version_label": expected,
        "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "scope": "Actual PTY keys and VT frames; private HOME/XDG/workspace and strict loopback provider."}
    fixture_parent = "/private/tmp" if Path("/private/tmp").is_dir() else None
    provider = Provider()
    try:
        with tempfile.TemporaryDirectory(prefix="ca-version-display-pty-", dir=fixture_parent) as directory:
            root = Path(directory).resolve()
            root.chmod(0o700)
            version_runtime = root / "version-runtime"
            version_runtime.mkdir(mode=0o700)
            cli = subprocess.run([str(binary), "--version"], cwd=root,
                env=isolated_environment(version_runtime, root), capture_output=True,
                text=True, timeout=10)
            receipt["cli_version"] = {"exit_code": cli.returncode, "stdout": cli.stdout,
                "stderr": cli.stderr, "matches_package": cli.returncode == 0
                and cli.stdout.strip() == "ca " + package_version}
            receipt["cases"] = {
                "interactive": run_case(binary, root / "interactive", provider, expected, output),
                "standalone-personality": run_case(binary, root / "standalone", provider, expected,
                                                   output, standalone=True)}
            requests = provider.snapshot()
            receipt["provider_requests"] = requests
            receipt["provider_failures"] = list(provider.failures)
            receipt["provider_request_contract_passed"] = (len(requests) == 1
                and requests[0]["user"] == PROMPT and requests[0]["response_sent"]
                and not provider.failures)
    except Exception as error:
        receipt["error"] = str(error)
    finally:
        provider.close()
    receipt["binary_sha256_after"] = binary_digest(binary)
    receipt["binary_identity_stable"] = receipt["binary_sha256"] == receipt["binary_sha256_after"]
    receipt["status"] = "passed" if (receipt["binary_identity_stable"]
        and receipt.get("cli_version", {}).get("matches_package")
        and receipt.get("provider_request_contract_passed") and receipt.get("cases")
        and all(case["status"] == "passed" for case in receipt["cases"].values())
        and not receipt.get("error")) else "failed"
    receipt["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"status": receipt["status"], "receipt": str(output / "receipt.json"),
        "binary_identity_stable": receipt["binary_identity_stable"],
        "cases": {name: {key: value for key, value in case.items()
            if key not in ("frames", "failure_screen")} for name, case in receipt.get("cases", {}).items()}},
        ensure_ascii=False), flush=True)
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
