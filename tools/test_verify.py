"""Behavioral contracts for the verification runner itself."""
import importlib.util
import hashlib
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

MODULE = Path(__file__).resolve().parents[1] / "scripts/verify.py"
spec = importlib.util.spec_from_file_location("verify_runner", MODULE)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class RunnerTests(unittest.TestCase):
    def test_source_inventory_includes_untracked_criteria_but_not_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("src/new.rs", "tools/test_verify_new.py", "crates/a/target/noise"):
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("initial")
            before, files = verify.source_inventory(root)
            self.assertEqual(set(files), {"src/new.rs", "tools/test_verify_new.py"})
            (root / "crates/a/target/noise").write_text("changed")
            self.assertEqual(before, verify.source_inventory(root)[0])
            (root / "tools/test_verify_new.py").write_text("changed")
            self.assertNotEqual(before, verify.source_inventory(root)[0])

    def test_environment_does_not_inherit_credentials_or_snapshot_acceptance(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.dict(os.environ, {"OPENAI_API_KEY": "test-sentinel", "SSH_AUTH_SOCK": "sentinel",
                                         "INSTA_UPDATE": "always", "CA_BIN": "live",
                                         "CARGO_HOME": str(Path(directory) / "empty-cache")}):
                env = verify.isolated_environment(Path(directory))
            for key in ("OPENAI_API_KEY", "SSH_AUTH_SOCK", "CA_BIN"):
                self.assertNotIn(key, env)
            self.assertEqual(env["INSTA_UPDATE"], "no")
            self.assertEqual(env["HOME"], str(Path(directory) / "home"))
            self.assertNotEqual(env["CARGO_HOME"], str(Path.home() / ".cargo"))
            import shutil
            shutil.rmtree(env["TMPDIR"])

    def test_actual_subprocess_failure_keeps_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            result = verify.run_stage("defect", [sys.executable, "-c", "print('expected 2, observed 1'); raise SystemExit(1)"],
                                      Path(directory), dict(os.environ), 5)
            self.assertEqual(result["status"], "failed")
            self.assertIn("expected 2, observed 1", Path(result["log"]).read_text())
            self.assertEqual(result["returncode"], 1)

    def test_fixture_temp_root_is_private_and_outside_sandbox_allowed_tmp(self):
        import shutil
        with tempfile.TemporaryDirectory() as directory:
            with patch.dict(os.environ, {"CARGO_HOME": str(Path(directory) / "empty-cache")}):
                env = verify.isolated_environment(Path(directory))
            root = Path(env["TMPDIR"]).resolve()
            try:
                self.assertNotIn(Path("/tmp").resolve(), (root, *root.parents))
                self.assertEqual(root.stat().st_mode & 0o777, 0o700)
                # Leave room for a tempfile child and a Unix socket filename.
                self.assertLess(len(str(root / ".tmp123456" / "rtorrent.sock")), 100)
            finally:
                shutil.rmtree(root)

    def test_dependency_bytes_are_real_private_files_without_user_cargo_settings(self):
        import shutil
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            cache = output / "user-cache"
            dependency = cache / "registry/src/example/lib.rs"
            dependency.parent.mkdir(parents=True)
            dependency.write_text("original dependency bytes")
            (cache / "config.toml").write_text("user-specific settings")
            (cache / "credentials.toml").write_text("test credential sentinel")
            with patch.dict(os.environ, {"CARGO_HOME": str(cache)}):
                env = verify.isolated_environment(output)
            try:
                private = Path(env["CARGO_HOME"])
                registry = private / "registry"
                self.assertTrue(registry.is_dir())
                self.assertFalse(registry.is_symlink())
                copied = registry / "src/example/lib.rs"
                self.assertEqual(copied.read_bytes(), dependency.read_bytes())
                copied.write_text("fixture mutation")
                self.assertEqual(dependency.read_text(), "original dependency bytes")
                self.assertFalse((private / "config.toml").exists())
                self.assertFalse((private / "credentials.toml").exists())
            finally:
                shutil.rmtree(env["TMPDIR"])

    def test_timeout_is_infrastructure_failure_and_is_bounded(self):
        with tempfile.TemporaryDirectory() as directory:
            result = verify.run_stage("timeout", [sys.executable, "-c", "import time; time.sleep(10)"],
                                      Path(directory), dict(os.environ), 0.05)
            self.assertEqual(result["status"], "infrastructure_failed")
            self.assertEqual(result["returncode"], 124)
            self.assertLess(result["duration_seconds"], 5)

    def test_interruption_terminates_child_after_parent_exits_and_freezes_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            child_file = output / "child.pid"
            group_file = output / "group.pid"
            command = """import os, signal, sys, time
from pathlib import Path
child_file = Path(sys.argv[1])
Path(sys.argv[2]).write_text(str(os.getpid()))
child = os.fork()
if child == 0:
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    child_file.write_text(str(os.getpid()))
    while True:
        print('same-group child still writing', flush=True)
        time.sleep(0.02)
else:
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    while True:
        time.sleep(0.02)
"""

            def child_is_live(pid):
                result = subprocess.run(["ps", "-o", "stat=", "-p", str(pid)],
                                        capture_output=True, text=True, check=False)
                status = result.stdout.strip()
                return bool(status) and not status.startswith("Z")

            try:
                with patch.object(verify, "GROUP_TERMINATION_GRACE", 0.15, create=True):
                    result = verify.run_stage("owned-child", [sys.executable, "-c", command,
                                              str(child_file), str(group_file)], output, dict(os.environ), 0.25)
                self.assertTrue(child_file.is_file(), "child must be running before the controlled timeout")
                pid = int(child_file.read_text())
                deadline = time.monotonic() + 1
                while child_is_live(pid) and time.monotonic() < deadline:
                    time.sleep(0.02)
                self.assertFalse(child_is_live(pid), "exited parent must not leave its TERM-ignoring child alive")
                self.assertEqual(result["status"], "infrastructure_failed")
                self.assertEqual(result["returncode"], 124)
                self.assertLess(result["duration_seconds"], 3)
                time.sleep(0.12)
                self.assertEqual(result["log_sha256"], hashlib.sha256(Path(result["log"]).read_bytes()).hexdigest(),
                                 "execution evidence must stop changing before its hash is recorded")
            finally:
                if group_file.is_file():
                    try:
                        os.killpg(int(group_file.read_text()), signal.SIGKILL)
                    except ProcessLookupError:
                        pass

    def test_compilation_and_infrastructure_failures_are_distinct(self):
        self.assertEqual(verify.classify(101, "error: invalid manifest", "preflight"), "failed")
        self.assertEqual(verify.classify(101, "no matching package named missing", "preflight"), "infrastructure_failed")
        self.assertEqual(verify.classify(101, "Cargo registry cache must be a real directory", "test"), "infrastructure_failed")
        self.assertEqual(verify.classify(101, "error[E0308]: mismatched types", "test"), "failed")
        self.assertEqual(verify.classify(101, "error: linking with cc failed: undefined symbol", "test"), "failed")
        self.assertEqual(verify.classify(101, "No space left on device", "test"), "infrastructure_failed")


if __name__ == "__main__":
    unittest.main()
