#!/usr/bin/env python3
"""Run verification and at most three explicitly configured, evidence-led repairs.

No model, provider, or editor is selected implicitly. A trusted repair command
receives CA_VERIFY_REPORT, CA_VERIFY_EVIDENCE, CA_VERIFY_ATTEMPT,
CA_VERIFY_REPAIR_NUMBER, CA_VERIFY_LOOP_OUTPUT, and CA_VERIFY_SOURCE_DIGEST.
The command must change implementation source without changing established
tests, evaluation criteria, verifier code, or earlier execution evidence.
"""

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time
import uuid


IGNORED_DIRS = {
    ".git", ".ca", ".codex", ".hermes", "target", "scratch", "oracles",
    "reference", "__pycache__", ".pytest_cache", "node_modules", "testai",
}
CRITERIA_DIRS = {"tests", "snapshots", "fixtures", "evals", "evaluations", "criteria"}
SOURCE_SUFFIXES = {".rs", ".py", ".sh", ".toml", ".lock"}
REPORT_SCHEMA = "ca.verify.v1"
LOOP_SCHEMA = "ca.verify-loop.v1"


class InfrastructureError(Exception):
    """Evidence or execution infrastructure could not be trusted."""


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def repository_files(root, excluded):
    """Include tracked and untracked inputs without reading runtime/secret data."""
    for directory, names, files in os.walk(root, followlinks=False):
        current = Path(directory)
        names[:] = sorted(
            name for name in names
            if name not in IGNORED_DIRS
            and not (current / name).is_symlink()
            and not any((current / name).resolve().is_relative_to(path) for path in excluded)
        )
        for name in sorted(files):
            path = current / name
            if name.startswith((".env", "._")) or name == ".DS_Store":
                continue
            if path.suffix in {".key", ".pem", ".log", ".profraw"}:
                continue
            if any(path.resolve().is_relative_to(item) for item in excluded):
                continue
            yield path


def file_bytes(path):
    # Hash a link's identity, never the contents of an external target.
    if path.is_symlink():
        return b"symlink\0" + os.fsencode(os.readlink(path))
    return path.read_bytes()


def rust_code_mask(text):
    """Blank comments/literals while preserving offsets for brace matching.

    This is a conservative lexical guard, not a Rust parser. Established
    #[cfg(test)] modules and test-attributed items are protected byte-for-byte.
    Dedicated test modules and fixtures are protected as complete files.
    """
    result = list(text)
    index = 0
    raw_pattern = re.compile(r'(?:br|cr|r)(#+)?"')
    character_pattern = re.compile(r"'(?:\\(?:[nrt0\\'\"]|x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]+\})|[^'\\\n])'")
    while index < len(text):
        end = index
        if text.startswith("//", index):
            end = text.find("\n", index)
            end = len(text) if end < 0 else end
        elif text.startswith("/*", index):
            end, depth = index + 2, 1
            while end < len(text) and depth:
                if text.startswith("/*", end):
                    depth += 1
                    end += 2
                elif text.startswith("*/", end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
        else:
            raw = raw_pattern.match(text, index)
            if raw and (index == 0 or not (text[index - 1].isalnum() or text[index - 1] == "_")):
                terminator = '"' + (raw.group(1) or "")
                last = text.find(terminator, raw.end())
                end = len(text) if last < 0 else last + len(terminator)
            elif text[index] == '"':
                end = index + 1
                while end < len(text):
                    if text[end] == "\\":
                        end += 2
                    elif text[end] == '"':
                        end += 1
                        break
                    else:
                        end += 1
            elif text[index] == "'":
                character = character_pattern.match(text, index)
                if character:
                    end = character.end()
        if end > index:
            result[index:end] = ["\n" if char == "\n" else " " for char in text[index:end]]
            index = end
        else:
            index += 1
    return "".join(result)


def rust_test_sections(text):
    masked = rust_code_mask(text)
    attributes = re.compile(r"#\s*\[(?:[^\[\]]|\[[^\[\]]*\])*\]")
    consumed = 0
    occurrences = {}
    for match in attributes.finditer(masked):
        if match.start() < consumed:
            continue
        attribute = match.group()
        if not (re.search(r"\b(?:cfg|cfg_attr)\s*\([^)]*\btest\b", attribute)
                or re.search(r"(?:^#\s*\[\s*|::)test\s*(?:\(|\])", attribute)):
            continue
        start = match.start()
        # Preserve preceding attributes such as #[ignore] as part of this item.
        for previous in reversed(list(attributes.finditer(masked, 0, start))):
            if masked[previous.end():start].strip():
                break
            start = previous.start()
        cursor = match.end()
        while True:
            following = attributes.match(masked, cursor + len(masked[cursor:]) - len(masked[cursor:].lstrip()))
            if not following:
                break
            cursor = following.end()
        item = re.search(r"\b(fn|mod|struct|enum|impl|static|const|use)\s+([^\s({;:=]+)", masked[cursor:])
        terminator = re.search(r"[;{]", masked[cursor:])
        if terminator is None:
            raise InfrastructureError("unterminated Rust test item")
        end = cursor + terminator.end()
        if masked[end - 1] == "{":
            depth = 1
            while end < len(masked) and depth:
                if masked[end] == "{":
                    depth += 1
                elif masked[end] == "}":
                    depth -= 1
                end += 1
            if depth:
                raise InfrastructureError("unbalanced Rust test item")
        identity = ":".join(item.groups()) if item else "item"
        ordinal = occurrences.get(identity, 0)
        occurrences[identity] = ordinal + 1
        yield f"{identity}:{ordinal}", text[start:end].encode("utf-8")
        consumed = end


def whole_file_protected(relative):
    name = relative.name
    return (
        bool(CRITERIA_DIRS.intersection(relative.parts))
        or name in {"Cargo.toml", "Cargo.lock", "test.rs", "test_support.rs"}
        or name.startswith("test_")
        or name.endswith(("_tests.rs", "_test.rs", ".snap", ".snap.new", ".ca"))
        or (relative.parts[0] == "scripts" and name.startswith("verify"))
        or (relative.parts[0] == "tools" and name.startswith(("verify", "test_verify")))
        or str(relative) in {"tools/orc258_pty.py", "tools/bug257_pty.py", ".codex/hooks.json"}
    )


def snapshots(root, excluded):
    source = hashlib.sha256()
    protected = {}
    for path in repository_files(root, excluded):
        relative = path.relative_to(root)
        implementation = path.suffix in SOURCE_SUFFIXES or relative.parts[0] == "prompts"
        if not implementation and not whole_file_protected(relative):
            continue
        data = file_bytes(path)
        if whole_file_protected(relative):
            protected[str(relative)] = sha256(data)
        elif path.suffix == ".rs" and not path.is_symlink():
            for identity, section in rust_test_sections(data.decode("utf-8")):
                protected[f"{relative}::rust-test::{identity}"] = sha256(section)
        if implementation and not whole_file_protected(relative):
            source.update(os.fsencode(str(relative)) + b"\0" + data + b"\0")
    hooks = root / ".codex" / "hooks.json"
    if hooks.is_file() or hooks.is_symlink():
        protected[".codex/hooks.json"] = sha256(file_bytes(hooks))
    return source.hexdigest(), protected


def contained_file(value, attempt_dir):
    if not isinstance(value, str) or not value:
        raise InfrastructureError("report contains a missing artifact path")
    path = Path(value)
    if not path.is_absolute():
        path = attempt_dir / path
    path = path.resolve()
    if not path.is_relative_to(attempt_dir.resolve()) or not path.is_file():
        raise InfrastructureError(f"report artifact is missing or outside its attempt: {value}")
    return path


def normalized_failure(text, root, attempt_dir):
    text = text.replace(str(attempt_dir), "<attempt>").replace(str(root), "<repo>")
    text = re.sub(r"\b\d{4}-\d\d-\d\d[T ][\d:.]+(?:Z| UTC|[+-]\d\d:\d\d)?", "<time>", text)
    text = re.sub(r"(thread '[^'\n]+' )\(\d+\)( panicked)", r"\1(<thread-id>)\2", text)
    text = re.sub(
        r"(?m)^(\s*Finished[^\n]*target\(s\) in )[\d.]+(?:ms|s|m|h)(?: [\d.]+(?:ms|s|m|h))*[ \t]*$",
        r"\1<duration>", text)
    text = re.sub(r"(?:/private)?/(?:var/)?tmp/ca-v-[A-Za-z0-9_-]+", "<verify-tmp>", text)
    text = re.sub(r"finished in [\d.]+s", "finished in <duration>", text)
    return text.strip()


def consume_evidence(attempt_dir, returncode, root):
    report_path = attempt_dir / "report.json"
    try:
        report = json.loads(report_path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise InfrastructureError(f"verifier did not produce a readable report: {error}") from error
    if not isinstance(report, dict) or report.get("schema") != REPORT_SCHEMA:
        raise InfrastructureError("verifier report has an unsupported schema")
    status = report.get("status")
    if status not in {"passed", "failed", "infrastructure_failed"}:
        raise InfrastructureError("verifier report has an invalid status")
    expected_exit = {"passed": 0, "failed": 1, "infrastructure_failed": 2}[status]
    if returncode != expected_exit:
        raise InfrastructureError(f"verifier exit {returncode} disagrees with report status {status}")
    if not isinstance(report.get("source_digest"), str) or not report["source_digest"]:
        raise InfrastructureError("verifier report has no source digest")
    if status == "passed" and report.get("complete") is not True:
        raise InfrastructureError("incomplete verification cannot pass")
    stages = report.get("stages")
    diagnostics = report.get("diagnostics")
    if not isinstance(stages, list) or not isinstance(diagnostics, list):
        raise InfrastructureError("verifier report has no stage list or diagnostic list")
    if not all(isinstance(item, str) for item in diagnostics):
        raise InfrastructureError("verifier diagnostics must be strings")
    evidence = {"report": str(report_path), "report_sha256": sha256(report_path.read_bytes()),
                "diagnostics": diagnostics, "artifacts": []}
    failure = []
    seen = set()
    for stage in stages:
        if not isinstance(stage, dict) or not isinstance(stage.get("id"), str) or stage["id"] in seen:
            raise InfrastructureError("verifier stages need unique string identifiers")
        seen.add(stage["id"])
        if stage.get("status") not in {"passed", "failed", "infrastructure_failed", "skipped"}:
            raise InfrastructureError(f"invalid status for stage {stage['id']}")
        if stage.get("status") == "skipped":
            if status == "passed":
                raise InfrastructureError("passed verifier report contains a skipped stage")
            continue
        if not isinstance(stage.get("returncode"), int):
            raise InfrastructureError(f"stage {stage['id']} has no integer exit status")
        log_path = contained_file(stage.get("log"), attempt_dir)
        data = log_path.read_bytes()
        tail = data[-65536:].decode("utf-8", errors="replace")
        evidence["artifacts"].append({"id": stage["id"], "path": str(log_path),
                                      "sha256": sha256(data), "tail": tail})
        if stage["status"] != "passed":
            failure.append({"id": stage["id"], "status": stage["status"],
                            "returncode": stage["returncode"],
                            "log": normalized_failure(tail, root, attempt_dir)})
    evaluator = report.get("evaluator")
    if evaluator is not None:
        if not isinstance(evaluator, dict):
            raise InfrastructureError("invalid evaluator artifact")
        if evaluator.get("report"):
            path = contained_file(evaluator["report"], attempt_dir)
            data = path.read_bytes()
            try:
                evaluation = json.loads(data)
            except ValueError as error:
                raise InfrastructureError("evaluator artifact is not JSON") from error
            evidence["artifacts"].append({"id": "evaluator", "path": str(path),
                                          "sha256": sha256(data), "report": evaluation})
    if status == "passed" and (not stages or not evaluator or evaluator.get("status") != "passed"):
        raise InfrastructureError("passed verification needs stages and a passed independent evaluator")
    failure.append({"diagnostics": [normalized_failure(item, root, attempt_dir) for item in diagnostics]})
    fingerprint = sha256(json.dumps(failure, sort_keys=True).encode("utf-8"))
    evidence["failure_fingerprint"] = fingerprint
    write_json(attempt_dir / "consumed-evidence.json", evidence)
    return report, evidence


def artifact_snapshot(directory):
    return {str(path.relative_to(directory)): sha256(file_bytes(path))
            for path in sorted(directory.rglob("*")) if path.is_file() or path.is_symlink()}


def changed_established(before, after):
    return sorted(key for key, value in before.items() if after.get(key) != value)


def run_repair(command, root, environment, log, timeout):
    """Own the shell's process group so a deadline cannot leave a repair running."""
    process = subprocess.Popen(command, shell=True, executable="/bin/sh", cwd=root,
                               env=environment, stdin=subprocess.DEVNULL, stdout=log,
                               stderr=subprocess.STDOUT, start_new_session=True)
    try:
        return process.wait(timeout=timeout)
    except (subprocess.TimeoutExpired, KeyboardInterrupt):
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()
        raise


def run_loop(args, root, output):
    summary = {"schema": LOOP_SCHEMA, "status": "infrastructure_failed", "reason": "starting",
               "output": str(output), "max_repairs": args.max_repairs,
               "repair_command_configured": args.repair_command is not None,
               "attempts": [], "repairs": []}
    summary_path = output / "loop-report.json"
    excluded = {output.resolve()}
    started = time.monotonic()

    def finish(status, reason, code):
        summary.update(status=status, reason=reason, elapsed_seconds=round(time.monotonic() - started, 3))
        write_json(summary_path, summary)
        print(f"{status}: {reason}\nEvidence: {summary_path}", flush=True)
        return code

    try:
        _, protected = snapshots(root, excluded)
        write_json(output / "protected-manifest.json", {"schema": "ca.verify-protection.v1", "files": protected})
        baseline_digest = sha256(file_bytes(args.baseline)) if args.baseline else None
        seen_failures = set()
        for attempt in range(args.max_repairs + 1):
            attempt_dir = output / f"attempt-{attempt}"
            command = [str(root / "scripts" / "verify.sh"), "--output", str(attempt_dir)]
            if args.baseline:
                command.extend(["--baseline", str(args.baseline)])
            before_source, before_protected = snapshots(root, excluded)
            changed = changed_established(protected, before_protected)
            if changed:
                summary["protected_changes"] = changed
                return finish("failed", "established tests or verification criteria changed", 1)
            print(f"Verification attempt {attempt}: {attempt_dir}", flush=True)
            with (output / f"verifier-{attempt}.log").open("wb") as log:
                result = subprocess.run(command, cwd=root, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
            report, evidence = consume_evidence(attempt_dir, result.returncode, root)
            after_source, after_protected = snapshots(root, excluded)
            if before_source != after_source or changed_established(before_protected, after_protected):
                return finish("infrastructure_failed", "source or established criteria changed during verification", 2)
            summary["attempts"].append({"attempt": attempt, "report": str(attempt_dir / "report.json"),
                                        "status": report["status"], "source_digest": report["source_digest"],
                                        "failure_fingerprint": evidence["failure_fingerprint"]})
            write_json(summary_path, summary)
            if report["status"] == "passed":
                return finish("passed", "complete verification passed", 0)
            if report["status"] == "infrastructure_failed":
                return finish("infrastructure_failed", "verifier infrastructure failed; repair was not attempted", 2)
            fingerprint = evidence["failure_fingerprint"]
            if fingerprint in seen_failures:
                return finish("failed", "the same failure recurred without new diagnostic evidence", 1)
            seen_failures.add(fingerprint)
            if not args.repair_command:
                return finish("failed", "verification failed; inspect consumed evidence and configure a trusted repair command", 1)
            if attempt == args.max_repairs:
                return finish("failed", "automatic repair limit reached", 1)
            repair_number = attempt + 1
            repair_log = output / f"repair-{repair_number}.log"
            environment = os.environ.copy()
            environment.update(CA_VERIFY_REPORT=str(attempt_dir / "report.json"),
                               CA_VERIFY_EVIDENCE=str(attempt_dir / "consumed-evidence.json"),
                               CA_VERIFY_ATTEMPT=str(attempt), CA_VERIFY_REPAIR_NUMBER=str(repair_number),
                               CA_VERIFY_LOOP_OUTPUT=str(output), CA_VERIFY_SOURCE_DIGEST=report["source_digest"])
            prior_artifacts = {
                f"{directory.name}/{path}": digest
                for directory in sorted(output.glob("attempt-*")) if directory.is_dir()
                for path, digest in artifact_snapshot(directory).items()
            }
            repair = {"repair": repair_number, "after_attempt": attempt, "log": str(repair_log),
                      "source_before": after_source}
            summary["repairs"].append(repair)
            print(f"Repair {repair_number}: using retained report and logs", flush=True)
            try:
                with repair_log.open("wb") as log:
                    repair_returncode = run_repair(args.repair_command, root, environment, log, args.repair_timeout)
                repair["returncode"] = repair_returncode
            except subprocess.TimeoutExpired:
                repair["status"] = "infrastructure_failed"
                return finish("infrastructure_failed", "repair command exceeded its timeout", 2)
            new_source, new_protected = snapshots(root, excluded)
            repair["source_after"] = new_source
            changed = changed_established(protected, new_protected)
            if changed:
                repair.update(status="rejected", protected_changes=changed)
                return finish("failed", "repair changed established tests or verification criteria; review the retained diff", 1)
            current_artifacts = {
                f"{directory.name}/{path}": digest
                for directory in sorted(output.glob("attempt-*")) if directory.is_dir()
                for path, digest in artifact_snapshot(directory).items()
            }
            changed_artifacts = changed_established(prior_artifacts, current_artifacts)
            if changed_artifacts:
                repair.update(status="rejected", evidence_changes=changed_artifacts)
                return finish("failed", "repair changed earlier verification evidence", 1)
            if args.baseline and (not args.baseline.is_file() or sha256(file_bytes(args.baseline)) != baseline_digest):
                repair["status"] = "rejected"
                return finish("failed", "repair changed the established performance baseline", 1)
            if repair_returncode:
                repair["status"] = "failed"
                return finish("failed", f"repair command failed with exit {repair_returncode}", 1)
            if after_source == new_source:
                repair["status"] = "unchanged"
                return finish("failed", "repair made no implementation source change; verification was not repeated", 1)
            repair["status"] = "completed"
            write_json(summary_path, summary)
        return finish("failed", "automatic repair limit reached", 1)
    except (InfrastructureError, OSError, UnicodeError) as error:
        return finish("infrastructure_failed", str(error), 2)
    except KeyboardInterrupt:
        return finish("infrastructure_failed", "interrupted; retained completed attempt evidence", 2)


def parse_arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, help="new artifact directory (default: scratch/verify-loop/<unique-run>)")
    parser.add_argument("--baseline", type=Path, help="known-good verifier report forwarded to each attempt")
    parser.add_argument("--repair-command", help="explicit trusted shell command; omitted means verification and evidence only")
    parser.add_argument("--max-repairs", type=int, choices=range(4), default=3, metavar="0..3")
    parser.add_argument("--repair-timeout", type=int, default=900, metavar="SECONDS", help="repair command deadline (default 900, range 1..3600)")
    args = parser.parse_args()
    if not 1 <= args.repair_timeout <= 3600:
        parser.error("--repair-timeout must be between 1 and 3600 seconds")
    if args.repair_command is not None and not args.repair_command.strip():
        parser.error("--repair-command must not be empty")
    return args


def main():
    args = parse_arguments()
    root = Path(__file__).resolve().parents[1]
    if args.baseline:
        args.baseline = args.baseline.resolve()
        if not args.baseline.is_file():
            print("infrastructure_failed: baseline report does not exist", file=sys.stderr)
            return 2
    timestamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    output = args.output or root / "scratch" / "verify-loop" / f"{timestamp}-{uuid.uuid4().hex[:8]}"
    output = output.resolve()
    if output.exists():
        print(f"infrastructure_failed: output directory must be new: {output}", file=sys.stderr)
        return 2
    output.mkdir(parents=True)
    return run_loop(args, root, output)


if __name__ == "__main__":
    sys.exit(main())
