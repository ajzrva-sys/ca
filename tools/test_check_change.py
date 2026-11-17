"""Focused contracts for the fast preparation/check runner; no Cargo execution."""
import contextlib
import fcntl
import hashlib
import importlib.util
import io
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("check_change", Path(__file__).resolve().parents[1] / "scripts/check-change.py")
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


class CheckChangeContracts(unittest.TestCase):
    def test_each_selector_requires_an_executed_successful_case(self):
        log = "test module::ignored_case ... ignored\ntest module::live_case ... ok\n"
        self.assertEqual(CHECK.unmatched_selectors(log, ["live_case"]), [])
        self.assertEqual(CHECK.unmatched_selectors(log, ["live_case", "ignored_case", "missing"]), ["ignored_case", "missing"])

    def test_source_sync_reuses_unchanged_files_and_removes_deleted_inputs(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            (root / "target").mkdir()
            source, slot = root / "inputs", root / "slot"
            source.mkdir()
            (source / "live.rs").write_text("one")
            files = {"live.rs": hashlib.sha256(b"one").hexdigest()}
            with patch.object(CHECK, "ROOT", root):
                self.assertEqual(CHECK.sync_source(source, slot, files), 1)
                before = (slot / "live.rs").stat().st_mtime_ns
                (slot / "stale.rs").write_text("removed")
                self.assertEqual(CHECK.sync_source(source, slot, files), 0)
                self.assertEqual((slot / "live.rs").stat().st_mtime_ns, before)
                self.assertFalse((slot / "stale.rs").exists())
                (source / "live.rs").write_text("two")
                self.assertEqual(CHECK.sync_source(source, slot, {"live.rs": hashlib.sha256(b"two").hexdigest()}), 1)
                self.assertEqual((slot / "live.rs").read_text(), "two")

    def test_environment_is_fresh_and_does_not_export_target_or_ambient_keys(self):
        with tempfile.TemporaryDirectory() as raw:
            output = Path(raw)
            with patch.dict(os.environ, {"UNRELATED_PROVIDER_API_KEY": "fixture-only", "CARGO_TARGET_DIR": "/fixture-target"}):
                env = CHECK.environment(output, output / "private-cargo")
            self.assertNotIn("UNRELATED_PROVIDER_API_KEY", env)
            self.assertNotIn("CARGO_TARGET_DIR", env)
            self.assertEqual(env["HOME"], str(output / "home"))
            self.assertFalse((Path(env["HOME"]) / ".config/ca/config.toml").exists())
            Path(env["TMPDIR"]).rmdir()

    def test_concurrent_run_fails_before_mutating_stable_source(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            state = root / "scratch/check-change"
            state.mkdir(parents=True)
            with (state / "run.lock").open("a") as lock:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                with patch.object(CHECK, "ROOT", root), patch.object(CHECK, "STATE", state), patch.object(sys, "argv", ["check-change", "--prepare-only", "--cargo-home", str(state / "cargo-home")]), contextlib.redirect_stderr(io.StringIO()):
                    with self.assertRaises(SystemExit) as raised:
                        CHECK.main()
                self.assertEqual(raised.exception.code, 2)
                self.assertFalse((state / "source").exists())

    def test_no_selector_fails_before_preparation(self):
        with patch.object(sys, "argv", ["check-change"]), contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as raised:
                CHECK.main()
            self.assertEqual(raised.exception.code, 2)

    def test_source_sync_rejects_unexpected_symlinks(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source, slot = root / "inputs", root / "slot"
            source.mkdir()
            slot.mkdir()
            (slot / "unsafe.rs").symlink_to(root / "outside")
            with self.assertRaisesRegex(ValueError, "unexpected symlink"):
                CHECK.sync_source(source, slot, {})


if __name__ == "__main__":
    unittest.main()
