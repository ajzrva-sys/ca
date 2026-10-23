#!/usr/bin/env python3
"""Optional trusted Codex SessionStart/Stop workflow reminders.

This hook never runs tests, edits implementation, invokes a repair command, or
installs an executable. Stop can request at most three continuations per turn.
Passing evidence is independently checked before it advances the session's
source baseline. Read-only sessions do not require a new verification run.
"""

import hashlib
import importlib.util
import json
from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[1]
MAX_CONTINUATIONS = 3
CONTEXT = (
    "For project changes, follow the repository verification workflow: run ./scripts/verify.sh, "
    "inspect its retained report/logs and independent evaluation, correct implementation from actual evidence, "
    "and retest. Automatic repairs are limited to three; established tests and evaluation criteria stay protected. "
    "Read-only work needs no new verification run. The optional hook needs project trust and /hooks review."
)


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def read_json(path):
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError("expected JSON object: " + str(path))
    return value


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    temporary = path.with_suffix(".json.pending")
    temporary.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def state_path(kind, identifier):
    return ROOT / "scratch" / "verify-hooks" / (kind + "-" + hashlib.sha256(identifier.encode()).hexdigest() + ".json")


def current_digest():
    verifier = load_module("ca_verify_hook_runner", ROOT / "scripts" / "verify.py")
    return verifier.source_inventory(ROOT)[0]


def latest_report(digest):
    candidates = list((ROOT / "scratch" / "verify").glob("*/report.json"))
    candidates.extend((ROOT / "scratch" / "verify-loop").glob("*/attempt-*/report.json"))
    candidates = [path for path in candidates if path.is_file() and not path.is_symlink()]
    for path in sorted(candidates, key=lambda item: item.stat().st_mtime_ns, reverse=True):
        try:
            report = read_json(path)
            if report.get("source_digest") == digest:
                return path, report
        except (OSError, ValueError):
            continue
    return None, None


def independently_passed(path, report, digest):
    if (report.get("schema") != "ca.verify.v1" or report.get("status") != "passed"
            or report.get("complete") is not True or report.get("source_digest_after") != digest):
        return False, "The matching verification report is failed or incomplete."
    evaluator = report.get("evaluator")
    if not isinstance(evaluator, dict) or evaluator.get("status") != "passed":
        return False, "The matching report has no passed independent evaluator."
    expected = path.parent / "evaluation.json"
    actual = Path(evaluator.get("report", ""))
    if actual.resolve() != expected.resolve() or actual.is_symlink() or expected.is_symlink():
        return False, "The evaluator artifact is missing or outside the matching run."
    try:
        evaluation = read_json(expected)
        module = load_module("ca_verify_hook_evaluator", ROOT / "tools" / "verify_evaluate.py")
        if (evaluation.get("schema") != module.SCHEMA or evaluation.get("status") != "passed"
                or evaluation.get("complete") is not True or evaluation.get("source_digest") != digest
                or evaluation.get("criteria") != module.CRITERIA):
            return False, "The saved independent evaluation does not match current source and criteria."
        assessment = module.assess(path)
        if assessment.get("status") != "passed" or assessment.get("complete") is not True:
            return False, "Retained execution evidence no longer passes independent evaluation."
        if report.get("baseline"):
            comparison = module.compare(assessment, module.assess(Path(report["baseline"])))
            if comparison.get("failures"):
                return False, "The retained baseline comparison fails."
        return True, ""
    except Exception as error:
        return False, "Independent evidence could not be evaluated: " + str(error)


def handle(payload):
    event = payload.get("hook_event_name")
    session = payload.get("session_id")
    if not isinstance(session, str) or not session or len(session) > 1024:
        print("[verify-hook] Missing session identity; workflow reminder was not initialized.", file=sys.stderr)
        return None
    session_path = state_path("session", session)
    if event == "SessionStart":
        # Resume/compaction emits SessionStart too. Preserve the original
        # baseline so an unverified source change cannot disappear at restart.
        if not session_path.is_file():
            save(session_path, {"schema": "ca.verify-hook-session.v1", "source_digest": current_digest()})
        return {"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": CONTEXT}}
    if event != "Stop":
        return None
    if not session_path.is_file():
        print("[verify-hook] No SessionStart baseline; after project changes run ./scripts/verify.sh.", file=sys.stderr)
        return None
    state = read_json(session_path)
    digest = current_digest()
    if state.get("source_digest") == digest:
        return None
    path, report = latest_report(digest)
    passed, explanation = independently_passed(path, report, digest) if report else (False, "No verification report matches the changed source.")
    if passed:
        state["source_digest"] = digest
        state["passed_report"] = str(path)
        state["continuations"] = 0
        save(session_path, state)
        return None
    turn = payload.get("turn_id")
    if not isinstance(turn, str) or not turn or len(turn) > 1024:
        print("[verify-hook] Missing turn identity; cannot enforce bounded continuation. " + explanation, file=sys.stderr)
        return None
    turn_path = state_path("turn", session + "\0" + turn)
    counter = read_json(turn_path) if turn_path.is_file() else {"continuations": 0}
    count = counter.get("continuations", 0)
    # A Stop continuation can start a new turn. Keep the chain's bound while
    # stop_hook_active is true rather than granting three more continuations.
    if payload.get("stop_hook_active") is True or state.get("continuation_turn") == turn:
        count = max(count, state.get("continuations", 0))
    if not isinstance(count, int) or count < 0:
        raise ValueError("invalid continuation counter")
    if count >= MAX_CONTINUATIONS:
        print("[verify-hook] Three continuations reached; report remaining verification failures honestly. " + explanation, file=sys.stderr)
        return None
    counter.update(schema="ca.verify-hook-turn.v1", continuations=count + 1, source_digest=digest)
    save(turn_path, counter)
    state.update(continuations=count + 1, continuation_turn=turn)
    save(session_path, state)
    diagnostics = "; ".join(str(item) for item in (report or {}).get("diagnostics", [])[:3])[:1500]
    reason = (
        f"Project source changed. {explanation} "
        "Run ./scripts/verify.sh and inspect actual retained logs/evaluation before correcting implementation. "
        "Do not repeat an unchanged failure or weaken established tests; stop and report a genuine external blocker "
        "or the three-repair limit. "
        + (f"Evidence: {path}. " if path else "")
        + (f"Diagnostics: {diagnostics}. " if diagnostics else "")
        + f"Workflow continuation {count + 1}/{MAX_CONTINUATIONS}."
    )
    return {"decision": "block", "reason": reason}


def main():
    response = None
    try:
        raw = sys.stdin.read(65537)
        if len(raw) > 65536:
            raise ValueError("hook payload exceeds 64 KiB")
        payload = json.loads(raw)
        if not isinstance(payload, dict):
            raise ValueError("hook payload must be a JSON object")
        response = handle(payload)
    except (OSError, ValueError, TypeError, KeyError, AttributeError, ImportError) as error:
        # Unavailable hooks never conceal verification failure or trap a turn.
        print("[verify-hook] Workflow reminder unavailable: " + str(error), file=sys.stderr)
    # The hook protocol expects a JSON response even when allowing Stop.
    print(json.dumps(response or {}), flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
