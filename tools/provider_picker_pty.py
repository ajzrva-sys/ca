#!/usr/bin/env python3
"""Exercise provider/model selection and catalog refresh with real PTY input.

python3 tools/provider_picker_pty.py --binary target/release/ca --receipt scratch/providers/terminal.json
Uses a private HOME/XDG/workspace and credentialless loopback catalog/replies.
Retains frames, raw terminal bytes, request routing and restoration evidence.
It does not contact a live provider or read user configuration or credentials.
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

from bug257_pty import Fixture, Screen


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class Catalog:
    def __init__(self):
        self.lock = threading.Lock()
        self.models = {"alpha": ["alpha-one", "alpha-two"], "beta": ["beta-old"]}
        self.status = {"alpha": 200, "beta": 200}
        self.delay = {"alpha": 0, "beta": 0}
        self.requests, self.failures = [], []
        catalog = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_GET(self):
                route = self.path.split("/", 2)[1]
                try:
                    assert self.path == f"/{route}/v1/models" and route in catalog.models
                    with catalog.lock:
                        status, models, delay = catalog.status[route], list(catalog.models[route]), catalog.delay[route]
                        catalog.requests.append({"method": "GET", "provider": route, "path": self.path, "status": status})
                    time.sleep(delay)
                    body = json.dumps({"data": [{"id": model} for model in models]}).encode()
                    self.send_response(status)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                except (BrokenPipeError, ConnectionResetError):
                    pass
                except Exception as error:
                    with catalog.lock:
                        catalog.failures.append(repr(error))
                    self.send_error(500)

            def do_POST(self):
                route = self.path.split("/", 2)[1]
                try:
                    assert route in catalog.models and self.path == f"/{route}/v1/chat/completions"
                    size = int(self.headers.get("Content-Length", "0"))
                    assert 0 < size < 4 * 1024 * 1024
                    payload = json.loads(self.rfile.read(size))
                    assert payload.get("stream")
                    prompt = next(message.get("content", "") for message in reversed(payload["messages"])
                                  if message.get("role") == "user")
                    with catalog.lock:
                        catalog.requests.append({"method": "POST", "provider": route, "path": self.path,
                                                 "model": payload["model"], "prompt": prompt})
                    reply = f"PROVIDER_FIXTURE_REPLY {route}/{payload['model']} {prompt}"
                    events = [{"choices": [{"index": 0, "delta": {"role": "assistant", "content": reply}, "finish_reason": None}]},
                              {"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                               "usage": {"prompt_tokens": 20, "completion_tokens": 10, "total_tokens": 30}}]
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream")
                    self.send_header("Connection", "close")
                    self.end_headers()
                    for event in events:
                        self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
                    self.wfile.write(b"data: [DONE]\n\n")
                    self.wfile.flush()
                    self.close_connection = True
                except Exception as error:
                    with catalog.lock:
                        catalog.failures.append(repr(error))
                    self.send_error(500)

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def url(self, route):
        return f"http://127.0.0.1:{self.server.server_port}/{route}/v1"

    def arm(self, route, models, *, status=200, delay=0):
        with self.lock:
            self.models[route], self.status[route], self.delay[route] = list(models), status, delay

    def count(self, method, route):
        with self.lock:
            return sum(request["method"] == method and request["provider"] == route for request in self.requests)

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)


class Terminal(Fixture):
    def __init__(self, binary, root, catalog):
        self.workspace, self.runtime = root / "workspace", root / "runtime"
        self.workspace.mkdir(mode=0o700)
        self.runtime.mkdir(mode=0o700)
        config = self.runtime / ".config/ca"
        config.mkdir(parents=True, mode=0o700)
        self.config = config / "config.toml"
        configuration = "trusted_workspaces = [" + json.dumps(str(self.workspace)) + "]\n"
        for route, models in (("alpha", ["alpha-one", "alpha-two"]), ("beta", ["beta-old"])):
            configuration += f"[providers.{route}]\nbase_url=" + json.dumps(catalog.url(route)) + "\napi_key=''\n"
            configuration += "models=" + json.dumps(models) + "\nprotocol='chat_completions'\n"
        configuration += "[defaults]\nprovider='alpha'\nmodel='alpha-one'\n"
        configuration += "[harness_improvement]\nenabled=false\n[learning.dream]\nenabled=false\n[learning.distill]\nenabled=false\n"
        self.config.write_text(configuration)
        self.config.chmod(0o600)
        self.environment = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(self.runtime),
                            "XDG_CONFIG_HOME": str(self.runtime / ".config"),
                            "XDG_DATA_HOME": str(self.runtime / ".local/share"),
                            "XDG_CACHE_HOME": str(self.runtime / ".cache"),
                            "TMPDIR": str(root), "TERM": "xterm-256color", "LANG": "en_US.UTF-8", "COLORFGBG": "15;0"}
        self.screen = Screen(120, 36)
        self.bytes, self.raw_tail, self.raw = 0, b"", bytearray()
        self.master, slave = pty.openpty()
        self.original_termios = termios.tcgetattr(slave)
        self.restored_termios, self.closed = False, False
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 120, 0, 0))

        def terminal_session():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        self.process = subprocess.Popen([str(binary), "--profile", "default", "--exclude-workspace-context",
                                         "--no-session", "--name", "Provider picker loopback fixture"],
                                        cwd=self.workspace, stdin=slave, stdout=slave, stderr=slave,
                                        env=self.environment, preexec_fn=terminal_session)
        os.close(slave)

    def read(self, timeout=0.05):
        prior = self.bytes
        result = super().read(timeout)
        if self.bytes > prior:
            self.raw.extend(self.raw_tail[-(self.bytes - prior):])
        return result

    def settle(self, seconds=0.2):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            assert self.read(0.02), f"ca exited {self.process.returncode}: {self.screen.text()}"

    def command(self, text, expect=None):
        self.write(b"\x1b[200~" + text.encode() + b"\x1b[201~\r")
        if expect:
            self.wait_text(expect, 8)
        self.settle()

    def key(self, value, expect=None):
        self.write(value)
        if expect:
            self.wait_text(expect, 8)
        self.settle()

    def until(self, predicate, message, timeout=8):
        deadline = time.monotonic() + timeout
        while not predicate():
            assert self.read(0.03), f"ca exited during {message}"
            assert time.monotonic() < deadline, f"{message}: {self.screen.text()}"
        self.settle()

    def close(self):
        if self.closed:
            return
        if self.process.poll() is None:
            # Close a modal first; each Esc is independent terminal input.
            for _ in range(3):
                self.write(b"\x1b")
                self.read(0.08)
            self.write(b"\x03")
            deadline = time.monotonic() + 5
            while self.process.poll() is None and time.monotonic() < deadline:
                self.read(0.03)
        if self.process.poll() is None:
            os.killpg(self.process.pid, signal.SIGTERM)
        self.process.wait(timeout=5)
        self.read(0)
        self.restored_termios = termios.tcgetattr(self.master) == self.original_termios
        os.close(self.master)
        self.closed = True


def frame(terminal, label, frames):
    terminal.settle()
    value = {"label": label, "columns": terminal.screen.width, "rows": terminal.screen.height,
             "text": terminal.screen.text()}
    frames.append(value)
    return value["text"]


def detail(terminal, route):
    terminal.command("/provider", "Providers")
    terminal.key(route.encode())
    terminal.key(b"\r", f"Provider: {route}")


def assert_selected(terminal, model):
    assert re.search(r"▸\s+" + re.escape(model) + r"(?:\s|$| ·)", terminal.screen.text()), \
        f"highlighted selection is not {model}: {terminal.screen.text()}"


def navigate_to(terminal, model):
    for _ in range(32):
        if re.search(r"▸\s+" + re.escape(model) + r"(?:\s|$| ·)", terminal.screen.text()):
            return
        terminal.key(b"\x1b[B")
    raise AssertionError(f"arrow navigation never highlighted {model}: {terminal.screen.text()}")


def highlighted_model(terminal):
    match = re.search(r"▸\s+((?:alpha-|beta-)[a-z-]+|gpt-6\.1-sol)(?:\s|$| ·)", terminal.screen.text())
    assert match, f"no visible highlighted fixture model: {terminal.screen.text()}"
    return match.group(1)


def prove_route(terminal, catalog, route, model, marker):
    before = len([request for request in catalog.requests if request["method"] == "POST"])
    terminal.command(marker, f"PROVIDER_FIXTURE_REPLY {route}/{model}")
    terminal.until(lambda: len([request for request in catalog.requests if request["method"] == "POST"]) > before,
                   "selected provider request not received")
    requests = [request for request in catalog.requests if request["method"] == "POST"][before:]
    assert len(requests) == 1 and requests[0]["provider"] == route and requests[0]["model"] == model, requests
    assert marker in requests[0]["prompt"], requests
    defaults = re.search(r"(?ms)^\[defaults\]\n(.*?)(?=^\[|\Z)", terminal.config.read_text())
    assert defaults and re.search(r"(?m)^provider\s*=\s*" + re.escape(json.dumps(route)) + r"\s*$", defaults.group(1))
    assert re.search(r"(?m)^model\s*=\s*" + re.escape(json.dumps(model)) + r"\s*$", defaults.group(1)), defaults.group(1)


def exercise(terminal, catalog, frames, checks):
    terminal.wait_text("alpha-one", 15)
    frame(terminal, "startup", frames)
    terminal.command("/model", "Switch model")
    terminal.key(b"alpha-two")
    terminal.key(b"\r")
    prove_route(terminal, catalog, "alpha", "alpha-two", "PICK_CONFIGURED_ALPHA")
    checks.append("Configured model selected with filter and Enter; request used alpha/alpha-two")
    frame(terminal, "configured-alpha-selected", frames)

    detail(terminal, "beta")
    assert_selected(terminal, "beta-old")
    frame(terminal, "configured-beta-detail", frames)
    terminal.key(b"\r")
    prove_route(terminal, catalog, "beta", "beta-old", "PICK_CONFIGURED_BETA")
    checks.append("Configured provider selected with filter and Enter; request used beta/beta-old")

    detail(terminal, "beta")
    catalog.arm("beta", ["beta-old", "gpt-6.1-sol"], delay=2)
    before = catalog.count("GET", "beta")
    terminal.key(b"R")
    terminal.until(lambda: catalog.count("GET", "beta") > before, "provider-detail R did not refresh")
    terminal.key(b"beta-")
    terminal.resize(80, 24)
    terminal.settle(0.1)
    pending_frame = frame(terminal, "detail-refresh-responsive-narrow", frames)
    assert "> beta-" in pending_frame and "refreshing" in pending_frame, "refresh blocked filter/resize input"
    terminal.until(lambda: "live · 2 models" in terminal.screen.text(), "detail refresh did not finish")
    terminal.resize(120, 36)
    terminal.settle()
    terminal.key(b"\x7f" * 5)
    terminal.wait_text("gpt-6.1-sol", 8)
    terminal.key(b"gpt-6.1-sol")
    assert_selected(terminal, "gpt-6.1-sol")
    frame(terminal, "new-live-model-visible", frames)
    terminal.key(b"\r")
    prove_route(terminal, catalog, "beta", "gpt-6.1-sol", "PICK_LIVE_61_SOL")
    checks.append("Provider detail refreshed, new gpt-6.1-sol visible/selectable and used on beta endpoint")

    detail(terminal, "beta")
    terminal.key(b"gpt-6.1-sol")
    catalog.arm("beta", [], status=503)
    before = catalog.count("GET", "beta")
    terminal.key(b"R")
    terminal.until(lambda: catalog.count("GET", "beta") > before, "failed catalog request absent")
    terminal.wait_text("HTTP 503", 8)
    assert_selected(terminal, "gpt-6.1-sol")
    frame(terminal, "failed-refresh-retains-selection", frames)
    catalog.arm("beta", ["beta-new", "beta-old", "gpt-6.1-sol"])
    before = catalog.count("GET", "beta")
    terminal.key(b"R")
    terminal.until(lambda: catalog.count("GET", "beta") > before, "detail refresh retry absent")
    terminal.until(lambda: "live · 3 models" in terminal.screen.text(), "detail retry did not recover")
    assert_selected(terminal, "gpt-6.1-sol")
    frame(terminal, "retry-retains-selection", frames)
    terminal.key(b"\r")
    prove_route(terminal, catalog, "beta", "gpt-6.1-sol", "PICK_AFTER_RETRY")
    checks.append("HTTP 503 preserved model/filter/selection; R retry recovered and retained selected route")

    terminal.command("/model", "Switch model")
    terminal.key(b"beta-new")
    catalog.arm("beta", ["beta-live", "beta-new", "beta-old", "gpt-6.1-sol"])
    before = {route: catalog.count("GET", route) for route in ("alpha", "beta")}
    terminal.key(b"R")
    terminal.until(lambda: all(catalog.count("GET", route) > count for route, count in before.items()),
                   "model picker did not refresh all providers")
    terminal.until(lambda: "2 providers live" in terminal.screen.text(), "global catalog refresh did not finish")
    assert_selected(terminal, "beta-new")
    frame(terminal, "picker-refresh-preserves-filter", frames)
    terminal.key(b"\r")
    prove_route(terminal, catalog, "beta", "beta-new", "PICK_GLOBAL_REFRESH")
    checks.append("Model picker R refreshed every configured provider and preserved filtered selection")

    terminal.command("/model", "Switch model")
    navigate_to(terminal, "beta-live")
    frame(terminal, "unfiltered-live-model-highlighted", frames)
    catalog.arm("beta", ["beta-a-first", "beta-live", "beta-new", "beta-old", "gpt-6.1-sol"])
    before = {route: catalog.count("GET", route) for route in ("alpha", "beta")}
    terminal.key(b"R")
    terminal.until(lambda: all(catalog.count("GET", route) > count for route, count in before.items()),
                   "unfiltered model picker did not refresh providers")
    terminal.until(lambda: "7 models · 2 providers live" in terminal.screen.text(), "unfiltered catalog refresh did not finish")
    assert_selected(terminal, "beta-live")
    frame(terminal, "unfiltered-refresh-preserves-model-identity", frames)
    terminal.key(b"\r")
    prove_route(terminal, catalog, "beta", "beta-live", "PICK_REORDERED_CATALOG")
    checks.append("Unfiltered catalog insertion retained live-only highlighted model identity and selected route")

    detail(terminal, "beta")
    terminal.key(b"\x1b[H")
    navigate_to(terminal, "beta-a-first")
    frame(terminal, "live-only-model-highlighted-before-removal", frames)
    catalog.arm("beta", ["beta-live", "beta-old", "gpt-6.1-sol"])
    before = catalog.count("GET", "beta")
    terminal.key(b"R")
    terminal.until(lambda: catalog.count("GET", "beta") > before, "shrinking catalog refresh absent")
    terminal.until(lambda: "live · 3 models" in terminal.screen.text(), "shrinking catalog refresh did not finish")
    assert "beta-a-first" not in terminal.screen.text(), "removed live-only model remains visible"
    selected_after_removal = highlighted_model(terminal)
    frame(terminal, "removed-model-falls-back-to-visible-selection", frames)
    terminal.key(b"\r")
    prove_route(terminal, catalog, "beta", selected_after_removal, "PICK_AFTER_REMOVAL")
    checks.append("Catalog removal discarded live-only row; Enter selected exactly the remaining visible highlight")

    terminal.command("/provider", "Providers")
    terminal.key(b"beta")
    catalog.arm("beta", ["beta-a-first", "beta-extra", "beta-live", "beta-new", "beta-old", "gpt-6.1-sol"])
    before = catalog.count("GET", "beta")
    terminal.key(b"R")
    terminal.until(lambda: catalog.count("GET", "beta") > before, "provider-list R did not refresh")
    assert any("▸ beta" in line for line in terminal.screen.text().splitlines()), "provider-list filter/selection lost"
    frame(terminal, "provider-list-refresh-preserves-filter", frames)
    terminal.key(b"\r", "Provider: beta")
    terminal.wait_text("beta-extra", 8)
    frame(terminal, "provider-list-refreshed-models", frames)
    terminal.key(b"\x1b")
    terminal.key(b"\x1b")
    posts = sum(request["method"] == "POST" for request in catalog.requests)
    terminal.key(b"PROVIDER_EDITABLE_DRAFT", "PROVIDER_EDITABLE_DRAFT")
    frame(terminal, "usable-after-picker-dismissal", frames)
    assert sum(request["method"] == "POST" for request in catalog.requests) == posts, "draft was submitted"
    checks.append("Provider list R refreshed catalog, retained provider/filter and returned to editable draft")


def exercise_scoped_then_global(terminal, catalog, frames, checks, *, pending=False):
    terminal.wait_text("alpha-one", 15)
    terminal.command("/provider setup beta", "Provider setup")
    terminal.key(b"\r", "Provider setup: beta")
    catalog.arm("beta", ["beta-old", "gpt-6.1-sol"], delay=3 if pending else 0)
    terminal.key(b"R")
    terminal.until(lambda: catalog.count("GET", "beta") == 1, "initial scoped refresh absent")
    if not pending:
        terminal.until(lambda: "live" in terminal.screen.text() and "Refreshing" not in terminal.screen.text(),
                       "initial scoped refresh did not finish")
    assert catalog.count("GET", "alpha") == 0, "provider setup refresh unexpectedly queried other providers"
    frame(terminal, "pending-scoped-refresh-only-beta" if pending else "scoped-refresh-only-beta", frames)
    checks.append("Provider setup detail R contacted only selected configured provider")
    terminal.key(b"\x1b")
    terminal.key(b"\x1b")
    catalog.arm("alpha", ["alpha-one", "alpha-two", "alpha-live"])
    terminal.command("/model", "Switch model")
    if pending:
        frame(terminal, "global-picker-open-while-scoped-refresh-pending", frames)
    terminal.until(lambda: catalog.count("GET", "alpha") >= 1,
                   "scoped refresh incorrectly suppressed first global model discovery")
    terminal.key(b"alpha-live")
    terminal.until(lambda: re.search(r"▸\s+alpha-live(?:\s|$)", terminal.screen.text()),
                   "unvisited provider's live-only model was not selectable")
    frame(terminal, "scoped-followed-by-global-discovery", frames)
    terminal.key(b"\r")
    prove_route(terminal, catalog, "alpha", "alpha-live", "PICK_PREVIOUSLY_UNVISITED_PROVIDER")
    checks.append("Initial scoped refresh did not suppress global catalog discovery or cross-provider selection")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--scenario", choices=("workflow", "scoped-then-global", "pending-scoped-then-global"), default="workflow")
    args = parser.parse_args()
    binary, receipt = args.binary.resolve(strict=True), args.receipt.resolve()
    receipt.parent.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="ca-provider-pty-", dir="/private/var/tmp" if Path("/private/var/tmp").exists() else "/var/tmp")).resolve()
    os.chmod(root, 0o700)
    result = {"status": "running", "root": str(root), "scenario": args.scenario, "binary": str(binary), "binary_sha256": digest(binary),
              "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(), "checks": [], "frames": []}
    catalog, terminal = Catalog(), None
    try:
        terminal = Terminal(binary, root, catalog)
        if args.scenario == "workflow":
            exercise(terminal, catalog, result["frames"], result["checks"])
        else:
            exercise_scoped_then_global(terminal, catalog, result["frames"], result["checks"],
                                       pending=args.scenario == "pending-scoped-then-global")
        assert not catalog.failures, catalog.failures
        result["status"] = "passed"
    except Exception as error:
        result["status"], result["error"] = "failed", str(error)
        if terminal:
            result["frames"].append({"label": "failure", "text": terminal.screen.text()})
    finally:
        if terminal:
            terminal.close()
            result["exit_code"] = terminal.process.returncode
            result["terminal_restored"] = terminal.restored_termios
            result["terminal_bytes"] = terminal.bytes
            raw = root / "terminal.vt"
            raw.write_bytes(terminal.raw)
            raw.chmod(0o600)
            result["raw_terminal"] = {"path": str(raw), "sha256": digest(raw)}
            if result["status"] == "passed" and (terminal.process.returncode != 0 or not terminal.restored_termios):
                result["status"], result["error"] = "failed", "child exit or terminal restoration failed"
        catalog.close()
        result["requests"], result["fixture_failures"] = catalog.requests, catalog.failures
        result["binary_sha256_after"] = digest(binary)
        if result["binary_sha256"] != result["binary_sha256_after"]:
            result["status"], result["error"] = "failed", "binary changed during validation"
        result["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        for target in (root / "result.json", receipt):
            target.write_text(json.dumps(result, indent=2) + "\n")
            target.chmod(0o600)
        summary = {key: result.get(key) for key in ("status", "root", "binary_sha256", "exit_code", "terminal_restored")}
        summary["checks_passed"] = len(result["checks"])
        if result.get("error"):
            summary["error"] = result["error"][:240]
        print(json.dumps(summary))
    raise SystemExit(0 if result["status"] == "passed" else 1)


if __name__ == "__main__":
    main()
