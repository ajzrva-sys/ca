#!/usr/bin/env python3
"""Disposable tests for optional Codex hook messages and continuation bounds."""

import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "verify-hook.py"
FAKE_RUNNER = '''
import hashlib
def source_inventory(root):
    return hashlib.sha256((root / "source.rs").read_bytes()).hexdigest(), {}
'''
FAKE_EVALUATOR = '''
import hashlib
import json
from pathlib import Path
SCHEMA = "ca.verify.evaluation.v1"
CRITERIA = "fixture-v1"
def assess(path):
    report = json.loads(path.read_text())
    passed = report["status"] == "passed" and report["complete"]
    for stage in report.get("stages", []):
        log = Path(stage["log"])
        passed = passed and log.is_file() and hashlib.sha256(log.read_bytes()).hexdigest() == stage["log_sha256"]
    return {"status": "passed" if passed else "failed", "complete": passed}
def compare(current, baseline):
    return {"failures": [] if baseline["status"] == "passed" else ["baseline failed"]}
'''


class HookContract(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="ca-verify-hook-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        (self.root / "scripts").mkdir()
        (self.root / "tools").mkdir()
        shutil.copy2(SCRIPT, self.root / "scripts" / "verify-hook.py")
        (self.root / "scripts" / "verify.py").write_text(FAKE_RUNNER)
        (self.root / "tools" / "verify_evaluate.py").write_text(FAKE_EVALUATOR)
        (self.root / "source.rs").write_text("initial")

    def event(self, event="Stop", session="session", turn="turn", **values):
        payload = {"hook_event_name": event, "session_id": session, "turn_id": turn, **values}
        result = subprocess.run([sys.executable, str(self.root / "scripts" / "verify-hook.py")],
                                input=json.dumps(payload), text=True, capture_output=True, cwd=self.root, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(result.stdout.strip(), "hook must emit a JSON response")
        response = json.loads(result.stdout) or None
        return response, result.stderr

    def start_and_change(self):
        response, _ = self.event("SessionStart")
        self.assertEqual(response["hookSpecificOutput"]["hookEventName"], "SessionStart")
        (self.root / "source.rs").write_text("changed")

    def report(self, name="passing", status="passed", evaluation=True, digest=None, loop=False):
        directory = self.root / "scratch" / ("verify-loop" if loop else "verify") / name
        if loop:
            directory /= "attempt-0"
        directory.mkdir(parents=True)
        digest = digest or hashlib.sha256((self.root / "source.rs").read_bytes()).hexdigest()
        log = directory / "behavior.log"
        log.write_text("observed actual behavior")
        report = {"schema": "ca.verify.v1", "source_digest": digest, "source_digest_after": digest,
                  "status": status, "complete": True, "diagnostics": ["actual failure"] if status != "passed" else [],
                  "stages": [{"id": "behavior", "status": status, "returncode": 0 if status == "passed" else 1,
                              "log": str(log), "log_sha256": hashlib.sha256(log.read_bytes()).hexdigest()}],
                  "evaluator": {"status": status, "report": str(directory / "evaluation.json")}}
        (directory / "report.json").write_text(json.dumps(report))
        if evaluation:
            (directory / "evaluation.json").write_text(json.dumps({"schema": "ca.verify.evaluation.v1",
                "status": status, "complete": status == "passed", "source_digest": digest, "criteria": "fixture-v1"}))
        return directory

    def test_read_only_session_needs_no_verification(self):
        response, _ = self.event("SessionStart")
        self.assertIn("./scripts/verify.sh", response["hookSpecificOutput"]["additionalContext"])
        response, _ = self.event()
        self.assertIsNone(response)
        self.assertFalse(list((self.root / "scratch" / "verify-hooks").glob("turn-*")))

    def test_changed_source_requires_actual_verification(self):
        self.start_and_change()
        response, _ = self.event()
        self.assertEqual(response["decision"], "block")
        self.assertIn("./scripts/verify.sh", response["reason"])
        self.assertIn("No verification report matches", response["reason"])

    def test_continuations_stop_after_three_even_when_hook_active(self):
        self.start_and_change()
        for count in range(1, 4):
            response, _ = self.event(stop_hook_active=True)
            self.assertEqual(response["decision"], "block")
            self.assertIn(f"{count}/3", response["reason"])
        response, error = self.event(stop_hook_active=True)
        self.assertIsNone(response)
        self.assertIn("Three continuations reached", error)

    def test_new_turn_has_independent_bound(self):
        self.start_and_change()
        for _ in range(4):
            self.event()
        response, _ = self.event(turn="second")
        self.assertEqual(response["decision"], "block")
        self.assertIn("1/3", response["reason"])

    def test_hook_continuation_chain_cannot_reset_bound_with_new_turn_ids(self):
        self.start_and_change()
        for count in range(1, 4):
            response, _ = self.event(turn=f"continuation-{count}", stop_hook_active=count > 1)
            self.assertEqual(response["decision"], "block")
            self.assertIn(f"{count}/3", response["reason"])
        response, error = self.event(turn="continuation-4", stop_hook_active=True)
        self.assertIsNone(response)
        self.assertIn("Three continuations reached", error)

    def test_compact_and_resume_do_not_discard_unverified_source_delta(self):
        self.start_and_change()
        for source in ("compact", "resume"):
            with self.subTest(source=source):
                response, _ = self.event("SessionStart", source=source)
                self.assertEqual(response["hookSpecificOutput"]["hookEventName"], "SessionStart")
                response, _ = self.event(turn=source)
                self.assertEqual(response["decision"], "block")

    def test_pass_is_independently_evaluated_then_updates_baseline(self):
        self.start_and_change()
        directory = self.report()
        response, _ = self.event()
        self.assertIsNone(response)
        (directory / "behavior.log").unlink()
        response, _ = self.event()
        self.assertIsNone(response)  # No further source change after accepted verification.
        (self.root / "source.rs").write_text("next change")
        response, _ = self.event(turn="next")
        self.assertEqual(response["decision"], "block")

    def test_modified_execution_log_rejects_implementer_pass_claim(self):
        self.start_and_change()
        directory = self.report()
        (directory / "behavior.log").write_text("rewritten")
        response, _ = self.event()
        self.assertEqual(response["decision"], "block")
        self.assertIn("no longer passes independent evaluation", response["reason"])

    def test_bare_pass_without_evaluation_does_not_pass(self):
        self.start_and_change()
        self.report(evaluation=False)
        response, _ = self.event()
        self.assertEqual(response["decision"], "block")
        self.assertIn("could not be evaluated", response["reason"])

    def test_failed_report_provides_evidence_and_diagnostics(self):
        self.start_and_change()
        directory = self.report(status="failed")
        response, _ = self.event()
        self.assertEqual(response["decision"], "block")
        self.assertIn(str(directory / "report.json"), response["reason"])
        self.assertIn("actual failure", response["reason"])

    def test_stale_pass_does_not_verify_changed_source(self):
        self.start_and_change()
        self.report(digest="old source")
        response, _ = self.event()
        self.assertEqual(response["decision"], "block")

    def test_loop_attempt_pass_can_verify_current_source(self):
        self.start_and_change()
        self.report(loop=True)
        response, _ = self.event()
        self.assertIsNone(response)

    def test_uninitialized_or_unidentifiable_turn_is_reminder_only(self):
        response, error = self.event()
        self.assertIsNone(response)
        self.assertIn("No SessionStart baseline", error)
        self.start_and_change()
        response, error = self.event(turn=None)
        self.assertIsNone(response)
        self.assertIn("Missing turn identity", error)

    def test_saved_evaluation_must_use_current_criteria(self):
        self.start_and_change()
        directory = self.report()
        path = directory / "evaluation.json"
        value = json.loads(path.read_text())
        value["criteria"] = "weakened"
        path.write_text(json.dumps(value))
        response, _ = self.event()
        self.assertEqual(response["decision"], "block")
        self.assertIn("current source and criteria", response["reason"])


if __name__ == "__main__":
    unittest.main()
