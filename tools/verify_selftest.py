#!/usr/bin/env python3
"""Prove pass/real production defect/restore against an unchanged existing test."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("verify_runner", ROOT / "scripts/verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)
TEST = "types::tests::validates_pid_interval_destination_and_paths"
FILE = "crates/ca-watch/src/types.rs"
GOOD = b"pub const MIN_INTERVAL_SECS: u64 = 2;"
BAD = b"pub const MIN_INTERVAL_SECS: u64 = 1;"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    output = (args.output or ROOT / "scratch/verify-selftest" / str(os.getpid())).resolve()
    try:
        output.mkdir(parents=True, exist_ok=False, mode=0o700)
    except OSError as error:
        print("Self-test evidence directory unavailable: " + str(error), file=sys.stderr)
        return 2
    parent = "/private/tmp" if Path("/private/tmp").is_dir() else None
    copy = Path(tempfile.mkdtemp(prefix="ca-defect-", dir=parent)).resolve()
    receipt = {"schema": "ca.verify.selftest.v1", "status": "running", "candidate": str(copy),
               "test": TEST, "production_file": FILE, "phases": [],
               "scope": "Focused validator demonstration in a disposable source copy; the full clean workflow is separate."}
    original = None
    try:
        digest, files = verify.source_inventory(ROOT)
        receipt["original_source_digest"] = digest
        for relative in files:
            destination = copy / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / relative, destination)
        env = verify.isolated_environment(output)
        env["CARGO_TARGET_DIR"] = str(ROOT / "scratch/verify-selftest/target")
        cargo = shutil.which("cargo", path=env["PATH"])
        if not cargo:
            receipt["status"] = "infrastructure_failed"
            raise RuntimeError("Cargo unavailable")
        command = [cargo, "test", "--locked", "--offline", "-p", "ca-watch", "--lib", TEST,
                   "--", "--exact", "--test-threads=1"]
        verify.ROOT = copy
        source = copy / FILE
        original = source.read_bytes()
        if original.count(GOOD) != 1:
            raise RuntimeError("Controlled defect precondition changed; update demonstration after reviewing existing criteria")
        for name in ("baseline", "intentional-defect", "restored"):
            if name == "intentional-defect":
                source.write_bytes(original.replace(GOOD, BAD))
            elif name == "restored":
                source.write_bytes(original)
            stage = verify.run_stage(name, command, output, env, 600, "test")
            data = Path(stage["log"]).read_text(errors="replace")
            if name == "intentional-defect":
                valid = (stage["status"] == "failed" and stage["returncode"] == 101
                         and TEST + " ... FAILED" in data and "assertion failed" in data
                         and "0 passed; 1 failed" in data)
            else:
                valid = (stage["status"] == "passed" and TEST + " ... ok" in data
                         and "1 passed; 0 failed" in data)
            stage["expected_outcome_observed"] = valid
            receipt["phases"].append(stage)
            if not valid:
                if stage["status"] == "infrastructure_failed":
                    receipt["status"] = "infrastructure_failed"
                raise RuntimeError("Unexpected " + name + " outcome; inspect " + stage["log"])
        restored, _ = verify.source_inventory(copy)
        receipt["restored_source_digest"] = restored
        current, _ = verify.source_inventory(ROOT)
        receipt["original_source_unchanged"] = current == digest
        receipt["candidate_restored_exactly"] = restored == digest
        if not receipt["original_source_unchanged"] or restored != digest:
            raise RuntimeError("Source changed during demonstration")
        receipt["status"] = "passed"
    except (OSError, RuntimeError, ValueError) as error:
        if receipt["status"] != "infrastructure_failed":
            receipt["status"] = "infrastructure_failed" if isinstance(error, OSError) else "failed"
        receipt["error"] = str(error)
    finally:
        if original is not None:
            (copy / FILE).write_bytes(original)
        verify.write_json(output / "result.json", receipt)
    print(json.dumps({"status": receipt["status"], "result": str(output / "result.json"), "candidate": str(copy)}))
    return {"passed": 0, "failed": 1, "infrastructure_failed": 2}[receipt["status"]]


if __name__ == "__main__":
    raise SystemExit(main())
