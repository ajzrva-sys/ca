#!/usr/bin/env python3
"""Disposable subprocess contract tests for the bounded repair workflow."""

import importlib.util
import json
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "verify-loop.py"
FAKE_VERIFIER = r'''
import argparse
import hashlib
import json
from pathlib import Path
import sys

parser = argparse.ArgumentParser()
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--baseline")
args = parser.parse_args()
root = Path.cwd()
spec = json.loads((root / "fake-spec.json").read_text())
counter = root / "calls"
calls = int(counter.read_text()) + 1 if counter.exists() else 1
counter.write_text(str(calls))
args.output.mkdir()
source = (root / "src" / "implementation.rs").read_text()
status = spec.get("status", "failed")
if spec.get("pass_when") and spec["pass_when"] in source:
    status = "passed"
code = {"passed": 0, "failed": 1, "infrastructure_failed": 2}[status]
detail = source if spec.get("unique_evidence") else "assertion failed: actual=broken expected=fixed"
log = args.output / "behavior.log"
log.write_text("behavior: " + detail)
evaluation = args.output / "evaluation.json"
evaluation.write_text(json.dumps({"status": status, "observed": detail}))
report = {"schema": "ca.verify.v1", "status": status,
          "complete": spec.get("complete", True),
          "source_digest": hashlib.sha256(source.encode()).hexdigest(),
          "stages": [{"id": "behavior", "status": status, "returncode": code, "log": str(log)}],
          "diagnostics": [] if status == "passed" else ["behavior assertion failed"],
          "evaluator": {"status": status, "report": str(evaluation)}}
if args.baseline:
    report["baseline"] = args.baseline
if spec.get("malformed"):
    report["schema"] = "unexpected"
if spec.get("outside_log"):
    report["stages"][0]["log"] = str(root / "fake-spec.json")
(args.output / "report.json").write_text(json.dumps(report))
sys.exit(code)
'''


class RepairLoopContract(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="ca-verify-loop-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        for name in ("scripts", "tools", "src", "tests", "snapshots"):
            (self.root / name).mkdir()
        shutil.copy2(SCRIPT, self.root / "scripts" / "verify-loop.py")
        (self.root / "scripts" / "fake.py").write_text(FAKE_VERIFIER)
        (self.root / "scripts" / "verify.sh").write_text(
            "#!/bin/sh\nexec " + shlex.quote(sys.executable) + " scripts/fake.py \"$@\"\n")
        (self.root / "scripts" / "verify.sh").chmod(0o755)
        (self.root / "src" / "implementation.rs").write_text(
            '// broken\nfn behavior() {}\n#[cfg(test)]\nmod tests {\n'
            '    #[test]\n    fn established() { assert!(true); }\n}\n')
        (self.root / "tests" / "established.rs").write_text("#[test]\nfn established() { assert!(true); }\n")
        (self.root / "src" / "unit_tests.rs").write_text("#[test]\nfn established() { assert!(true); }\n")
        (self.root / "snapshots" / "behavior.snap").write_text("expected output\n")
        (self.root / "tools" / "verify_evaluate.py").write_text("EXPECTED = 'fixed'\n")
        (self.root / "Cargo.toml").write_text('[package]\nname="fixture"\nversion="0.1.0"\n')
        self.output = self.root / "scratch" / "result"
        self.spec()

    def spec(self, **values):
        (self.root / "fake-spec.json").write_text(json.dumps(values))

    def python_command(self, code):
        return shlex.quote(sys.executable) + " -c " + shlex.quote(code)

    def execute(self, command=None, *arguments):
        args = [sys.executable, str(self.root / "scripts" / "verify-loop.py"), "--output", str(self.output)]
        if command is not None:
            args.extend(["--repair-command", command])
        args.extend(arguments)
        result = subprocess.run(args, cwd=self.root, text=True, capture_output=True, timeout=15)
        summary = json.loads((self.output / "loop-report.json").read_text()) if (self.output / "loop-report.json").exists() else None
        return result, summary

    def test_failure_consumes_evidence_repairs_then_passes(self):
        self.spec(pass_when="fixed")
        command = self.python_command(
            'import json, os; from pathlib import Path; '
            'e=json.loads(Path(os.environ["CA_VERIFY_EVIDENCE"]).read_text()); '
            'assert e["artifacts"][0]["tail"].startswith("behavior:"); '
            'assert Path(os.environ["CA_VERIFY_REPORT"]).is_file(); '
            'assert os.environ["CA_VERIFY_ATTEMPT"] == "0"; '
            'assert os.environ["CA_VERIFY_REPAIR_NUMBER"] == "1"; '
            'p=Path("src/implementation.rs"); p.write_text(p.read_text().replace("// broken", "// fixed"))')
        result, summary = self.execute(command)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(summary["status"], "passed")
        self.assertEqual([a["status"] for a in summary["attempts"]], ["failed", "passed"])
        self.assertEqual(len(summary["repairs"]), 1)
        self.assertTrue((self.output / "attempt-0" / "consumed-evidence.json").is_file())
        self.assertTrue((self.output / "attempt-0" / "behavior.log").is_file())

    def test_limit_is_three_repairs_and_four_verifications(self):
        self.spec(unique_evidence=True)
        command = self.python_command(
            'import os; from pathlib import Path; p=Path("src/implementation.rs"); '
            'p.write_text(p.read_text()+"\\n// repair "+os.environ["CA_VERIFY_REPAIR_NUMBER"])')
        result, summary = self.execute(command)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertEqual(summary["reason"], "automatic repair limit reached")
        self.assertEqual(len(summary["repairs"]), 3)
        self.assertEqual(len(summary["attempts"]), 4)
        self.assertEqual((self.root / "calls").read_text(), "4")

    def test_unchanged_repair_is_not_reverified(self):
        result, summary = self.execute("true")
        self.assertEqual(result.returncode, 1)
        self.assertIn("no implementation source change", summary["reason"])
        self.assertEqual((self.root / "calls").read_text(), "1")

    def test_same_failure_with_source_change_does_not_retry_repair(self):
        command = self.python_command(
            'from pathlib import Path; p=Path("src/implementation.rs"); p.write_text(p.read_text()+"\\n// attempted fix")')
        result, summary = self.execute(command)
        self.assertEqual(result.returncode, 1)
        self.assertIn("same failure recurred", summary["reason"])
        self.assertEqual(len(summary["attempts"]), 2)
        self.assertEqual(len(summary["repairs"]), 1)

    def test_infrastructure_failure_stops_without_repair(self):
        self.spec(status="infrastructure_failed")
        result, summary = self.execute("exit 88")
        self.assertEqual(result.returncode, 2)
        self.assertEqual(summary["status"], "infrastructure_failed")
        self.assertEqual(len(summary["repairs"]), 0)
        self.assertEqual((self.root / "calls").read_text(), "1")

    def test_no_command_preserves_actionable_failure(self):
        result, summary = self.execute()
        self.assertEqual(result.returncode, 1)
        self.assertIn("inspect consumed evidence", summary["reason"])
        self.assertEqual(len(summary["repairs"]), 0)
        consumed = json.loads((self.output / "attempt-0" / "consumed-evidence.json").read_text())
        self.assertIn("assertion failed", consumed["artifacts"][0]["tail"])

    def test_preexisting_criteria_and_verifier_cannot_be_changed(self):
        for relative in ("tests/established.rs", "src/unit_tests.rs", "snapshots/behavior.snap",
                         "tools/verify_evaluate.py", "scripts/verify.sh", "Cargo.toml"):
            with self.subTest(relative=relative):
                self.output = self.root / "scratch" / relative.replace("/", "-")
                original = (self.root / relative).read_bytes()
                command = self.python_command(
                    f'from pathlib import Path; p=Path({relative!r}); p.write_text(p.read_text()+"\\n# altered")')
                result, summary = self.execute(command)
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("verification criteria", summary["reason"])
                self.assertEqual(summary["repairs"][0]["protected_changes"], [relative])
                (self.root / relative).write_bytes(original)

    def test_inline_test_assertions_are_protected(self):
        command = self.python_command(
            'from pathlib import Path; p=Path("src/implementation.rs"); '
            'p.write_text(p.read_text().replace("assert!(true)", "assert!(false)"))')
        result, summary = self.execute(command)
        self.assertEqual(result.returncode, 1)
        self.assertIn("rust-test::mod:tests", summary["repairs"][0]["protected_changes"][0])
        self.assertEqual(len(summary["attempts"]), 1)

    def test_established_failure_evidence_cannot_be_rewritten(self):
        command = self.python_command(
            'import os; from pathlib import Path; Path(os.environ["CA_VERIFY_REPORT"]).write_text("{}"); '
            'p=Path("src/implementation.rs"); p.write_text(p.read_text()+"\\n// fix")')
        result, summary = self.execute(command)
        self.assertEqual(result.returncode, 1)
        self.assertIn("earlier verification evidence", summary["reason"])
        self.assertEqual(summary["repairs"][0]["evidence_changes"], ["attempt-0/report.json"])

    def test_incomplete_pass_is_infrastructure_failure(self):
        self.spec(status="passed", complete=False)
        result, summary = self.execute()
        self.assertEqual(result.returncode, 2)
        self.assertIn("incomplete verification", summary["reason"])

    def test_malformed_or_outside_artifact_is_infrastructure_failure(self):
        for spec in ({"malformed": True}, {"outside_log": True}):
            with self.subTest(spec=spec):
                self.spec(**spec)
                self.output = self.root / "scratch" / next(iter(spec))
                result, summary = self.execute()
                self.assertEqual(result.returncode, 2)
                self.assertEqual(summary["status"], "infrastructure_failed")

    def test_baseline_is_forwarded_to_each_attempt(self):
        baseline = self.root / "baseline.json"
        baseline.write_text("{}")
        self.spec(status="passed")
        result, _ = self.execute(None, "--baseline", str(baseline))
        self.assertEqual(result.returncode, 0)
        report = json.loads((self.output / "attempt-0" / "report.json").read_text())
        self.assertEqual(report["baseline"], str(baseline.resolve()))

    def test_performance_baseline_cannot_be_rewritten_by_repair(self):
        baseline = self.root / "baseline.json"
        baseline.write_text('{"known_good":true}')
        command = self.python_command(
            'from pathlib import Path; Path("baseline.json").write_text("{}"); '
            'p=Path("src/implementation.rs"); p.write_text(p.read_text()+"\\n// fix")')
        result, summary = self.execute(command, "--baseline", str(baseline))
        self.assertEqual(result.returncode, 1)
        self.assertIn("performance baseline", summary["reason"])

    def test_zero_repairs_runs_once(self):
        result, summary = self.execute("exit 88", "--max-repairs", "0")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(summary["reason"], "automatic repair limit reached")
        self.assertEqual(len(summary["repairs"]), 0)

    def test_existing_output_is_rejected_without_verification(self):
        self.output.mkdir(parents=True)
        result, summary = self.execute()
        self.assertEqual(result.returncode, 2)
        self.assertIsNone(summary)
        self.assertFalse((self.root / "calls").exists())

    def test_repair_timeout_stops_without_reverification(self):
        result, summary = self.execute(self.python_command("import time; time.sleep(30)"), "--repair-timeout", "1")
        self.assertEqual(result.returncode, 2)
        self.assertIn("exceeded its timeout", summary["reason"])
        self.assertEqual(len(summary["attempts"]), 1)


class RustProtectionLexing(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location("ca_verify_loop", SCRIPT)
        cls.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.module)

    def test_strings_comments_raw_literals_and_lifetimes_do_not_break_protection(self):
        source = """fn implementation<'a>(value: &'a str) {}\n#[cfg(test)]\nmod tests {
// } misleading comment
/* { nested /* } */ comment */
#[test]
fn established() { let a = r###"} {"###; let b = "}"; let c = '}'; assert!(true); }
}
fn after() {}"""
        sections = list(self.module.rust_test_sections(source))
        self.assertEqual(len(sections), 1)
        self.assertTrue(sections[0][1].endswith(b"\n}"))
        self.assertNotIn(b"fn after", sections[0][1])

    def test_ignore_attribute_is_part_of_established_test(self):
        source = "#[ignore]\n#[tokio::test]\nasync fn established() { assert!(true); }"
        sections = list(self.module.rust_test_sections(source))
        self.assertEqual(len(sections), 1)
        self.assertTrue(sections[0][1].startswith(b"#[ignore]"))

    def test_failure_fingerprint_ignores_owned_run_identifiers_and_durations(self):
        first = (
            "    Finished `test` profile [unoptimized + debuginfo] target(s) in 4.80s\n"
            "thread 'tests::behavior' (12345) panicked at src/main.rs:10:9:\n"
            "assertion `left == right` failed\n  left: 3\n right: 4\n"
            "fixture: /private/tmp/ca-v-first123/workspace/result\n"
            "test result: FAILED. 0 passed; 1 failed; finished in 1.02s\n")
        second = first.replace("4.80s", "1m 12s").replace("12345", "67890").replace(
            "ca-v-first123", "ca-v-second456").replace("1.02s", "0.89s")
        root = Path("/repo")
        attempt = Path("/repo/scratch/verify-loop/run/attempt-0")
        normalize = lambda value: self.module.normalized_failure(value, root, attempt)
        self.assertEqual(normalize(first), normalize(second))
        for parent in ("/tmp", "/var/tmp", "/private/var/tmp"):
            with self.subTest(parent=parent):
                alternate = second.replace("/private/tmp/ca-v-second456", parent + "/ca-v-third789")
                self.assertEqual(normalize(first), normalize(alternate))
                self.assertNotEqual(normalize(first), normalize(alternate.replace("left: 3", "left: 2")))
        self.assertNotEqual(normalize(first), normalize(second.replace("left: 3", "left: 2")))
        self.assertNotEqual(normalize(first), normalize(second.replace("tests::behavior", "tests::other")))


if __name__ == "__main__":
    unittest.main()
