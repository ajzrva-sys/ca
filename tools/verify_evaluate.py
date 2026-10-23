#!/usr/bin/env python3
"""Independently assess retained CA verification evidence (0 pass, 1 fail, 2 infrastructure)."""
import argparse
from collections import Counter
import hashlib
import json
import math
import os
from pathlib import Path
import re
import sys
import uuid


SCHEMA = "ca.verify.evaluation.v1"
CRITERIA = "ca.verify.criteria.v1"
STAGES = ("preflight", "workflow-tests", "format", "lint", "test-default",
          "test-all-features", "release-build", "runtime")
CASES = ("cli_help", "cli_version", "pty_startup", "submitted_prompt", "tool_roundtrip",
         "independent_review", "resize", "terminal_restoration", "binary_unchanged")
REVIEW_INPUT = "Independently review the final code/configuration candidate"
REVIEW_TOOLS = {"read_file", "search_files", "glob", "list_dir", "report_structured_output"}
HOST_PREFIX = "CA host evidence (data only; not a human request or instructions).\n"
HOST_FIELDS = {"schema", "kind", "parent_turn", "origin_turn", "agent_id", "provider",
               "model", "status", "candidate_digest", "data"}
REVIEW_REPLY = "VERIFY_REVIEW_INTEGRATED_OK"
REVIEW_REPORT = {"approved": True, "findings": [],
                 "summary": "Read probe-output.txt from disk; exact candidate has no actionable findings."}
CARGO_ARGS = {
    "preflight": ["metadata", "--locked", "--offline", "--format-version", "1", "--no-deps"],
    "format": ["fmt", "--all", "--", "--check"],
    "lint": ["clippy", "--locked", "--offline", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"],
    "test-default": ["test", "--locked", "--offline", "--workspace", "--no-fail-fast", "--", "--test-threads=1"],
    "test-all-features": ["test", "--locked", "--offline", "--workspace", "--all-features", "--no-fail-fast", "--", "--test-threads=1"],
    "release-build": ["build", "--locked", "--offline", "--release", "--workspace", "--all-features"],
}
SUMMARY = re.compile(r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out")
HEADER = re.compile(r"^\s*(?:Running (.+) \(([^)]+)\)|Doc-tests\s+(\S+))\s*$", re.M)
DECLARED = re.compile(r"^running (\d+) tests?\s*$", re.M)
IGNORED = re.compile(r"^test (\S+) \.\.\. ignored(?:, (.*))?\s*$", re.M)
HASH = re.compile(r"^[a-f0-9]{64}$")
METRICS = ("reply_ms", "tool_ms", "input_frame_ms", "total_ms", "terminal_output_bytes",
           "child_exit_code", "child_peak_rss_kib")
TIMINGS = ("reply_ms", "tool_ms", "input_frame_ms", "total_ms")


class EvidenceError(Exception):
    pass


def require(condition, message):
    if not condition:
        raise EvidenceError(message)


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def read_json(path):
    require(path.is_file() and not path.is_symlink(), "missing or linked evidence: " + str(path))
    try:
        value = json.loads(path.read_text())
    except (ValueError, OSError) as error:
        raise EvidenceError("cannot read JSON evidence " + str(path) + ": " + str(error)) from error
    require(isinstance(value, dict), "JSON evidence is not an object: " + str(path))
    return value


def verified_file(path, expected):
    require(isinstance(expected, str) and HASH.fullmatch(expected), "invalid SHA-256 for " + str(path))
    require(path.is_file() and not path.is_symlink(), "missing or linked evidence: " + str(path))
    require(digest(path) == expected, "SHA-256 mismatch: " + str(path))


def host_review_data(body):
    """Independently validate the exact low-priority review publication."""
    def unique_keys(pairs):
        value = {}
        for key, item in pairs:
            require(key not in value, "duplicate host review evidence JSON key")
            value[key] = item
        return value

    values = []
    for message in body.get("messages", []):
        content = message.get("content")
        if isinstance(content, str):
            text = content
        elif isinstance(content, list):
            text = "\n".join(part.get("text", "") for part in content if isinstance(part, dict))
        else:
            continue
        if not text.startswith(HOST_PREFIX):
            continue
        require(message.get("role") == "user" and isinstance(content, str),
                "host review evidence must be lower-priority User data")
        try:
            value = json.loads(text[len(HOST_PREFIX):], object_pairs_hook=unique_keys)
        except ValueError as error:
            raise EvidenceError("malformed host review evidence JSON") from error
        require(isinstance(value, dict) and set(value) == HOST_FIELDS
                and value.get("schema") == "ca.host-evidence.v1"
                and value.get("kind") == "independent_review", "unexpected host review evidence schema/kind/fields")
        try:
            turn = uuid.UUID(value.get("parent_turn"))
        except (ValueError, TypeError, AttributeError) as error:
            raise EvidenceError("host review evidence current-turn binding invalid") from error
        require(turn.int != 0 and str(turn) == value.get("parent_turn")
                and value.get("origin_turn") == str(turn), "host review evidence current/origin turn differs")
        require(all(value.get(key) is None for key in ("agent_id", "provider", "model"))
                and value.get("status") == "Approved"
                and isinstance(value.get("candidate_digest"), str) and HASH.fullmatch(value["candidate_digest"]),
                "host review evidence identity/route/status/candidate differs")
        data = value.get("data")
        require(isinstance(data, dict) and set(data) == set(REVIEW_REPORT)
                and data.get("approved") is True and data == REVIEW_REPORT,
                "host review evidence does not match the actual approval report")
        values.append(value)
    require(len(values) <= 1, "host review evidence published more than once")
    return values[0] if values else None


def runtime_user(body):
    evidence = host_review_data(body)
    users = [message.get("content") for message in body.get("messages", [])
             if message.get("role") == "user"
             and not (isinstance(message.get("content"), str) and message["content"].startswith(HOST_PREFIX))]
    require(not evidence or users == ["VERIFY_CHAT_REQUEST", "VERIFY_TOOL_REQUEST"],
            "host review evidence changed the original human request")
    return users[-1] if users else None


def owned_candidate_digest(candidate, turn):
    """Recompute the turn/root/path/before/final identity from captured disk bytes."""
    root = candidate.parent.resolve()
    value = hashlib.sha256()
    value.update(b"ca.automatic-review.v1:independent-readonly-exact-candidate")
    value.update(uuid.UUID(turn).bytes)
    value.update(str(uuid.uuid5(uuid.NAMESPACE_URL, str(root))).encode())
    value.update(os.fsencode(root))
    value.update(os.fsencode(candidate))
    value.update(b"missing\0present\0")
    value.update(hashlib.sha256(candidate.read_bytes()).digest())
    return value.hexdigest()


def matrix(log):
    """Use Cargo target boundaries; filtered child probes cannot hide a filtered target."""
    headers = list(HEADER.finditer(log))
    require(headers, "Cargo test log has no target sections")
    targets = []
    totals = dict(passed=0, failed=0, ignored=0, measured=0, filtered_out=0)
    excluded = 0
    failures = []
    seen = Counter()
    for index, header in enumerate(headers):
        section = log[header.end():headers[index + 1].start() if index + 1 < len(headers) else len(log)]
        if header.group(3):
            name = "Doc-tests " + header.group(3)
        else:
            artifact = re.sub(r"-[0-9a-f]{16}(?:\.exe)?$", "", Path(header.group(2)).name)
            name = header.group(1) + " [" + artifact + "]"
        # Different workspace packages can both contain tests/cli.rs. Cargo
        # prints no package label there; retain each occurrence independently.
        seen[name] += 1
        if seen[name] > 1:
            name += " #" + str(seen[name])
        summaries = list(SUMMARY.finditer(section))
        require(summaries, "Cargo target has no completed summary: " + name)
        for child in summaries[:-1]:
            counts = [int(x) for x in child.groups()[1:]]
            before = section[:child.start()].splitlines()[-6:]
            require(child.group(1) == "ok" and counts[:4] == [1, 0, 0, 0] and counts[4] > 0
                    and any(line == "test session_access::tests::child_writer_probe ... ok" for line in before),
                    "unexpected nested Rust summary in " + name)
            excluded += 1
        final = summaries[-1]
        counts = dict(zip(totals, (int(x) for x in final.groups()[1:])))
        declared = DECLARED.search(section)
        require(declared is not None, "Cargo target omitted declared test count: " + name)
        require(int(declared.group(1)) == sum(counts[key] for key in ("passed", "failed", "ignored", "measured")),
                "declared Rust test count disagrees with summary: " + name)
        ignores = [{"name": match.group(1), "reason": match.group(2) or ""} for match in IGNORED.finditer(section)]
        require(len(ignores) == counts["ignored"], "ignored test names disagree with summary: " + name)
        require(len({entry["name"] for entry in ignores}) == len(ignores), "duplicate ignored test names: " + name)
        if final.group(1) != "ok" or counts["failed"]:
            failures.append("Rust target failed: " + name)
        if counts["filtered_out"]:
            failures.append("Rust target filtered out tests: " + name)
        for key in totals:
            totals[key] += counts[key]
        targets.append({"target": name, "artifact": header.group(3) or artifact,
                        "kind": "doc" if header.group(3) else "unit" if header.group(1).startswith("unittests ") else "integration",
                        **counts, "ignored_tests": ignores})
    require(totals["passed"] > 0, "Cargo matrix ran no passing tests")
    return {**totals, "target_count": len(targets), "targets": targets,
            "excluded_nested_child_summaries": excluded}, failures


def expected_targets(metadata, all_features):
    require(isinstance(metadata.get("packages"), list) and isinstance(metadata.get("workspace_members"), list),
            "preflight omitted Cargo workspace metadata")
    expected = Counter()
    for package in metadata["packages"]:
        if package.get("id") not in metadata["workspace_members"]:
            continue
        for target in package.get("targets", []):
            if not target.get("test"):
                continue
            if target.get("required-features") and not all_features:
                # This repository's only conditional binary is opt-in and has
                # no default feature; do not fabricate it in the default run.
                defaults = package.get("features", {}).get("default", [])
                if not set(target["required-features"]) <= set(defaults):
                    continue
            kind = target.get("kind", [])
            name = target.get("name", "").replace("-", "_")
            if "lib" in kind or "proc-macro" in kind or "bin" in kind:
                expected[("unit", name)] += 1
            elif "test" in kind:
                expected[("integration", name)] += 1
            if ("lib" in kind or "proc-macro" in kind) and target.get("doctest"):
                expected[("doc", name)] += 1
    require(expected, "preflight workspace declares no test targets")
    return expected


def evaluate_runtime(path, report):
    runtime = read_json(path)
    require(runtime.get("schema") == "ca.verify.runtime.v1", "unsupported runtime receipt schema")
    failures = []
    require(runtime.get("status") in {"passed", "failed", "infrastructure_failed"}, "invalid runtime status")
    require(runtime.get("binary_sha256") == report.get("binary_sha256"), "runtime/report binary hashes disagree")
    require(runtime.get("binary") == report.get("binary"), "runtime/report binary paths disagree")
    cases = runtime.get("cases")
    require(isinstance(cases, list) and all(isinstance(case, dict) for case in cases), "invalid runtime case list")
    names = [case.get("name") for case in cases]
    require(len(names) == len(set(names)), "duplicate runtime cases")
    require(set(CASES) <= set(names), "runtime receipt omitted required behavior cases")
    for case in cases:
        require(case.get("status") in {"passed", "failed", "infrastructure_failed", "not_run"}, "invalid runtime case status")
        if case["status"] != "passed":
            failures.append("runtime " + str(case.get("name")) + ": " + str(case.get("error", case["status"])))
    # Partial failed runs retain their receipt without inventing missing metrics/artifacts.
    if runtime["status"] != "passed" or failures:
        return runtime, failures
    metrics = runtime.get("metrics")
    require(isinstance(metrics, dict), "runtime metrics missing")
    for name in METRICS:
        value = metrics.get(name)
        require(isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value) and value >= 0,
                "invalid or missing runtime metric: " + name)
    require(metrics["terminal_output_bytes"] > 0 and metrics["child_peak_rss_kib"] > 0,
            "runtime reported no terminal output or process RSS")
    require(metrics["child_exit_code"] == 0, "runtime child did not exit cleanly")
    artifacts = runtime.get("artifacts")
    hashes = runtime.get("artifact_sha256")
    require(isinstance(artifacts, dict) and isinstance(hashes, dict) and hashes, "runtime artifact hashes missing")
    for filename, expected in hashes.items():
        require(isinstance(filename, str) and Path(filename).is_absolute(), "artifact hash path must be absolute")
        verified_file(Path(filename), expected)
    root = Path(artifacts.get("root", ""))
    require(root.resolve() == path.parent.resolve(), "runtime artifact root disagrees with receipt location")
    expected_paths = [root / (name + extension) for name in ("help", "version") for extension in (".stdout", ".stderr")]
    for name in ("requests", "terminal", "tool_output"):
        require(isinstance(artifacts.get(name), str), "runtime artifact path missing: " + name)
        expected_paths.append(Path(artifacts[name]))
    frames = Path(artifacts.get("frames", ""))
    expected_paths += [frames / (name + ".txt") for name in
                       ("pty_startup", "submitted_prompt", "tool_roundtrip", "independent_review", "resize-narrow", "resize-wide", "final")]
    for filename in expected_paths:
        require(str(filename) in hashes, "runtime artifact omitted from hash manifest: " + str(filename))
    require(re.search(r"Usage:.*ca", (root / "help.stdout").read_text()), "help output contract absent")
    require(re.search(r"^ca \d+\.\d+\.\d+", (root / "version.stdout").read_text()), "version output contract absent")
    require(Path(artifacts["terminal"]).stat().st_size == metrics["terminal_output_bytes"], "terminal byte metric disagrees with raw artifact")
    raw = Path(artifacts["terminal"]).read_bytes()
    require(b"\x1b[?1049l" in raw and b"\x1b[?25h" in raw, "terminal restoration sequences absent")
    require(Path(artifacts["tool_output"]).read_bytes() == b"VERIFY_TOOL_DISK\n", "real tool output does not match contract")
    require("VERIFY_CHAT_OK" in (frames / "submitted_prompt.txt").read_text(), "submitted prompt reply frame missing")
    require("VERIFY_TOOL_OK" in (frames / "tool_roundtrip.txt").read_text(), "tool reply frame missing")
    for filename in ("resize-narrow.txt", "resize-wide.txt"):
        require("VERIFY_TOOL_OK" in (frames / filename).read_text(), "resize lost rendered tool reply")
    requests = read_json(Path(artifacts["requests"]))
    require(requests.get("protocol_errors") == [], "fixture reported protocol errors")
    require(all(entry.get("path") == "/v1/chat/completions" for entry in requests.get("requests", [])),
            "fixture requests used an unexpected protocol endpoint")
    entries = requests.get("requests", [])
    bodies = [entry.get("body", {}) for entry in entries]
    publications = [host_review_data(body) for body in bodies]
    chats = [body for body in bodies if runtime_user(body) == "VERIFY_CHAT_REQUEST"]
    tools = [body for body in bodies if runtime_user(body) == "VERIFY_TOOL_REQUEST" and host_review_data(body) is None]
    reviews = [entry for entry in entries if runtime_user(entry.get("body", {})) == REVIEW_INPUT]
    require(len(bodies) == 6 and len(chats) == 1 and len(tools) == 2 and len(reviews) == 2
            and publications[:-1] == [None] * 5 and publications[-1] is not None,
            "fixture request counts do not match chat/tool/review roundtrips")
    require([runtime_user(body) for body in bodies] == ["VERIFY_CHAT_REQUEST", "VERIFY_TOOL_REQUEST", "VERIFY_TOOL_REQUEST",
                                                REVIEW_INPUT, REVIEW_INPUT, "VERIFY_TOOL_REQUEST"], "fixture roundtrip ordering changed")
    require(all(body.get("stream") is True for body in bodies), "fixture requests did not use streaming")
    require(all(body.get("model") == "fixture-model" for body in bodies), "fixture requests used an unexpected model")
    require(any(tool.get("function", {}).get("name") == "write_file" for tool in tools[0].get("tools", [])),
            "write_file capability absent from actual request")
    results = [message for message in tools[1].get("messages", [])
               if message.get("role") == "tool" and message.get("tool_call_id") == "verify-write-file"]
    require(len(results) == 1 and "VERIFY_TOOL_DISK" in str(results[0].get("content")), "matching real tool result absent from continuation")
    require(all(isinstance(entry.get("response_completed_at"), str) and entry["response_completed_at"] for entry in entries),
            "fixture response was not served before continuation")
    evaluate_review(runtime, reviews, entries[-1], Path(artifacts["tool_output"]), frames, Path(artifacts["terminal"]))
    return runtime, failures


def evaluate_review(runtime, reviews, parent, candidate, frames, terminal):
    """Reconcile independent raw requests, returned calls, disk bytes and completion frame."""
    receipt = runtime.get("independent_review")
    require(isinstance(receipt, dict), "independent review receipt missing")
    require(receipt.get("request_count") == 2 and receipt.get("tool_names") == sorted(REVIEW_TOOLS),
            "independent review receipt scope disagrees")
    require(receipt.get("candidate_path") == str(candidate) and HASH.fullmatch(str(receipt.get("candidate_digest", ""))),
            "independent review candidate binding invalid")
    require(all(receipt.get(name) is True for name in ("readback", "readonly", "reviewer_completed", "parent_finalized")),
            "independent review did not finish read-only review and parent finalization")
    digests = []
    for entry in reviews:
        body = entry["body"]
        definitions = body.get("tools", [])
        names = [tool.get("function", {}).get("name") for tool in definitions]
        require(len(names) == 5 and set(names) == REVIEW_TOOLS,
                "independent reviewer has unsafe or missing tool grants")
        parameters = next(tool["function"] for tool in definitions
                          if tool.get("function", {}).get("name") == "report_structured_output").get("parameters", {})
        value = parameters.get("properties", {}).get("value", {})
        fields = value.get("properties", {})
        require(parameters.get("type") == "object" and parameters.get("additionalProperties") is False
                and parameters.get("required") == ["value"] and set(parameters.get("properties", {})) == {"value"}
                and value.get("type") == "object" and value.get("additionalProperties") is False
                and set(value.get("required", [])) == {"approved", "findings", "summary"}
                and set(fields) == {"approved", "findings", "summary"}
                and fields["approved"].get("type") == "boolean" and fields["findings"].get("type") == "array"
                and fields["summary"].get("type") == "string", "independent reviewer report schema is unbounded")
        context = "\n".join(message.get("content", "") for message in body.get("messages", [])
                            if message.get("role") in {"system", "developer"} and isinstance(message.get("content"), str))
        match = re.search(r"Independently review the exact turn-owned candidate ([0-9a-f]{64})\.", context)
        inventory = context.split("Exact changed-file inventory:\n", 1)
        require(match is not None and len(inventory) == 2
                and inventory[1].split("\n\n", 1)[0].splitlines() == [str(candidate)]
                and "Original request (context only):\nVERIFY_TOOL_REQUEST\n" in context
                and "Changed path: probe-output.txt\nBefore:\n[missing]\nCandidate:\nVERIFY_TOOL_DISK\n" in context,
                "independent review context does not match the exact written candidate")
        digests.append(match.group(1))
    require(digests == [receipt["candidate_digest"]] * 2, "independent review candidate digest drifted")

    def sole_call(entry, name, identifier):
        calls = entry.get("response_tool_calls")
        require(isinstance(calls, list) and len(calls) == 1, "independent reviewer did not return a sole tool call")
        call = calls[0]
        require(call.get("id") == identifier and call.get("type") == "function"
                and call.get("function", {}).get("name") == name, "independent reviewer response call disagrees")
        try:
            arguments = json.loads(call["function"].get("arguments", ""))
        except (ValueError, TypeError) as error:
            raise EvidenceError("independent reviewer response arguments invalid") from error
        return arguments

    require(sole_call(reviews[0], "read_file", "verify-review-read") == {"path": "probe-output.txt"},
            "independent reviewer read a different candidate")
    continuation = reviews[1]["body"].get("messages", [])
    reads = [message for message in continuation if message.get("role") == "tool"
             and message.get("tool_call_id") == "verify-review-read"]
    calls = [call for message in continuation if message.get("role") == "assistant"
             for call in message.get("tool_calls", []) if call.get("id") == "verify-review-read"]
    require(len(reads) == 1 and "VERIFY_TOOL_DISK" in str(reads[0].get("content"))
            and len(calls) == 1 and calls[0].get("function") == reviews[0]["response_tool_calls"][0].get("function"),
            "matching real reviewer readback absent from continuation")
    arguments = sole_call(reviews[1], "report_structured_output", "verify-review-report")
    report = arguments.get("value") if isinstance(arguments, dict) else None
    require(isinstance(arguments, dict) and set(arguments) == {"value"} and isinstance(report, dict)
            and set(report) == {"approved", "findings", "summary"} and report.get("approved") is True
            and report.get("findings") == [] and isinstance(report.get("summary"), str) and report["summary"].strip()
            and report == receipt.get("report"), "independent reviewer approval report envelope invalid")
    publication = host_review_data(parent["body"])
    require(publication is not None and publication == receipt.get("host_evidence")
            and publication["data"] == report and publication["candidate_digest"] == receipt["candidate_digest"],
            "final parent host evidence does not match the actual reviewer report")
    require(owned_candidate_digest(candidate, publication["parent_turn"]) == receipt["candidate_digest"],
            "final parent review evidence is not the exact current-turn disk candidate")
    require(type(receipt.get("parent_request_count")) is int and receipt["parent_request_count"] == 1
            and receipt.get("parent_reply") == REVIEW_REPLY
            and parent.get("response_tool_calls") == [] and parent.get("response_content") == REVIEW_REPLY,
            "final parent did not return the sole integrated-review text response")
    lines = (frames / "independent_review.txt").read_text().splitlines()
    require(any("Reviewer" in line and "Completed" in line for line in lines),
            "independent reviewer completion frame absent")
    require(REVIEW_REPLY in (frames / "independent_review.txt").read_text(),
            "final parent integrated-review reply frame absent")
    evidence = receipt.get("finalization_evidence")
    require(isinstance(evidence, dict) and evidence.get("status_row") == 36
            and evidence.get("terminal") == str(terminal)
            and evidence.get("completed_frame") == str(frames / "independent_review.txt"),
            "independent reviewer finalization artifact binding invalid")
    offset = evidence.get("report_response_offset")
    raw = terminal.read_bytes()
    require(isinstance(offset, int) and not isinstance(offset, bool) and 0 <= offset < len(raw)
            and reviews[1].get("response_terminal_offset") == offset,
            "independent reviewer final response offset invalid")
    parent_offset = evidence.get("parent_response_offset")
    require(type(parent_offset) is int and offset <= parent_offset < len(raw)
            and parent.get("response_terminal_offset") == parent_offset,
            "integrated-review parent response offset invalid")
    # The roster repaint may follow Done, and background status may replace it.
    # Only a real bottom-row status update after the final report proves completion.
    marker = re.compile(rb'\x1b\[36;1H(?:\x1b\[[0-?]*[ -/]*[@-~])*Done(?=\x1b|\s|$)')
    match = marker.search(raw, max(offset, parent_offset))
    require(match is not None and evidence.get("done_offset") == match.start(),
            "independent reviewer parent finalization absent after report response and parent response")


def assess(report_path):
    report_path = report_path.resolve()
    assessment = {"schema": SCHEMA, "criteria": CRITERIA, "status": "running", "report": str(report_path),
                  "diagnostics": [], "issues": [], "test_totals": {}, "metrics": {},
                  "limits": ["Retained evidence verifies a bounded local smoke run, not paid-provider effectiveness.",
                             "Performance comparison is a single-run smoke guard, not optimization proof.",
                             "Captured manifests bind the run; later edits to the working checkout are not evaluated."]}
    def issue(kind, message):
        assessment["issues"].append({"kind": kind, "message": message})
        assessment["diagnostics"].append(message)
    try:
        report = read_json(report_path)
        require(report.get("schema") == "ca.verify.v1", "unsupported verification report schema")
        require(isinstance(report.get("platform"), str) and report["platform"], "verification platform missing")
        assessment["platform"] = report["platform"]
        assessment["source_digest"] = report.get("source_digest")
        before = None
        for filename, key in (("source-manifest.json", "source_digest"), ("source-manifest-after.json", "source_digest_after")):
            manifest = read_json(report_path.parent / filename)
            require(manifest and all(isinstance(name, str) and isinstance(value, str) and HASH.fullmatch(value)
                                    for name, value in manifest.items()), "invalid source manifest: " + filename)
            encoded = json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode()
            require(hashlib.sha256(encoded).hexdigest() == report.get(key), "source manifest digest mismatch: " + filename)
            if key == "source_digest":
                before = manifest
        required_criteria = {"scripts/verify.py", "tools/verify_evaluate.py", "tools/verify_runtime.py"}
        require(required_criteria <= set(before), "source manifest omitted verification criteria")
        criteria = {name: value for name, value in before.items()
                    if name.startswith(("scripts/verify", "tools/verify", "tools/test_verify"))
                    or name in {"tools/orc258_pty.py", "tools/bug257_pty.py", ".codex/hooks.json"}}
        assessment["criteria_digest"] = hashlib.sha256(json.dumps(criteria, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        require(assessment["criteria_digest"] == report.get("criteria_digest"), "verification criteria digest mismatch")
        if report.get("source_digest") != report.get("source_digest_after"):
            issue("failed", "Source or evaluation criteria changed during verification")
        stages = report.get("stages")
        require(isinstance(stages, list) and all(isinstance(stage, dict) for stage in stages), "invalid stage list")
        ids = [stage.get("id") for stage in stages]
        require(len(ids) == len(set(ids)), "duplicate verification stages")
        require(not set(ids) - set(STAGES), "unknown verification stages")
        if ids != list(STAGES) or report.get("complete") is not True:
            known_failure = any(stage.get("status") in {"failed", "infrastructure_failed"} for stage in stages)
            expected_prefix = ids == list(STAGES[:len(ids)])
            if known_failure and expected_prefix and report.get("complete") is False:
                issue("infrastructure_failed" if any(stage.get("status") == "infrastructure_failed" for stage in stages)
                      else "failed", "Later verification stages did not run after a recorded failure")
            else:
                issue("infrastructure_failed", "Required verification stages were not completed in order")
        metadata = None
        for stage in stages:
            name = stage["id"]
            require(stage.get("status") in {"passed", "failed", "infrastructure_failed"}, "invalid stage status: " + name)
            require(isinstance(stage.get("returncode"), int) and not isinstance(stage["returncode"], bool), "missing stage return code: " + name)
            if stage["status"] != "passed" or stage["returncode"] != 0:
                issue(stage["status"] if stage["status"] != "passed" else "failed", name + " exited " + str(stage["returncode"]) + " (" + stage["status"] + ")")
            require((stage["returncode"] == 0) == (stage["status"] == "passed"), "stage status/return code contradiction: " + name)
            log = Path(stage.get("log", ""))
            require(log.resolve() == (report_path.parent / (name + ".log")).resolve(), "stage log path disagrees with run: " + name)
            verified_file(log, stage.get("log_sha256"))
            command = stage.get("command")
            require(isinstance(command, list) and command and all(isinstance(arg, str) for arg in command), "invalid stage command: " + name)
            if name in CARGO_ARGS:
                require(command[1:] == CARGO_ARGS[name], "required command arguments changed or tests filtered: " + name)
            if name == "workflow-tests":
                require(command[1:] == ["-m", "unittest", "discover", "-s", "tools", "-p", "test_verify*.py", "-v"],
                        "workflow test discovery command changed")
                if stage["status"] == "passed":
                    data = log.read_text(errors="replace")
                    count = re.search(r"^Ran (\d+) tests? in ", data, re.M)
                    require(count is not None and int(count.group(1)) > 0 and re.search(r"^OK\s*$", data, re.M)
                            and not re.search(r"\.\.\. skipped|OK \(skipped=", data),
                            "workflow checks omitted tests, skipped cases, or lack a passing unittest summary")
            if name == "preflight" and stage["status"] == "passed":
                metadata = read_json(log)
            if name in {"test-default", "test-all-features"}:
                try:
                    counts, failures = matrix(log.read_text(errors="replace"))
                except EvidenceError as error:
                    if stage["status"] == "passed":
                        raise
                    # A compilation failure occurs before a Rust test binary
                    # can emit summaries. It remains an implementation failure;
                    # the independently hashed diagnostic log is its evidence.
                    issue(stage["status"], name + ": test matrix not completed: " + str(error))
                    assessment["test_totals"][name] = {"complete": False, "targets": [], "diagnostic": str(error)}
                    continue
                assessment["test_totals"][name] = counts
                for failure in failures:
                    issue("failed", name + ": " + failure)
                if stage["status"] == "passed" and metadata is not None:
                    expected = expected_targets(metadata, name == "test-all-features")
                    observed = Counter((target["kind"], target["artifact"]) for target in counts["targets"])
                    require(expected == observed, "Cargo matrix target inventory disagrees with preflight: " + name)
                elif stage["status"] == "passed":
                    issue("failed", name + ": cannot establish target inventory after failed preflight")
        if "runtime" in ids:
            runtime_stage = next(stage for stage in stages if stage["id"] == "runtime")
            command = runtime_stage["command"]
            require(len(command) == 6 and Path(command[1]).name == "verify_runtime.py"
                    and command[2:] == ["--binary", report.get("binary"), "--output", str(report_path.parent / "runtime")],
                    "runtime command does not match retained candidate and evidence directory")
            verified_file(report_path.parent / "runtime/result.json", report.get("runtime_sha256"))
            verified_file(Path(report.get("binary", "")), report.get("binary_sha256"))
            runtime, failures = evaluate_runtime(report_path.parent / "runtime/result.json", report)
            assessment["metrics"] = runtime.get("metrics", {})
            for failure in failures:
                issue("infrastructure_failed" if runtime.get("status") == "infrastructure_failed" else "failed", failure)
        if report.get("status") not in {"passed", "failed", "infrastructure_failed"}:
            issue("infrastructure_failed", "Verification report has no completed status")
        elif report["status"] != "passed":
            issue(report["status"], "Verification runner status: " + report["status"])
    except (EvidenceError, OSError, TypeError, KeyError, ValueError) as error:
        issue("infrastructure_failed", str(error))
    kinds = [entry["kind"] for entry in assessment["issues"]]
    assessment["status"] = "infrastructure_failed" if "infrastructure_failed" in kinds else "failed" if kinds else "passed"
    assessment["complete"] = assessment["status"] == "passed"
    return assessment


def compare(current, baseline):
    require(baseline["status"] == "passed" and baseline.get("complete") is True, "baseline is not a complete passing run")
    require(current.get("platform") == baseline.get("platform"), "baseline platform is not comparable")
    require(current.get("criteria") == baseline.get("criteria"), "baseline evaluation criteria are not comparable")
    require(current.get("criteria_digest") == baseline.get("criteria_digest"), "baseline verification code/criteria changed")
    comparison = {"baseline_report": baseline["report"], "policy": "timings <= baseline * 2 + 100 ms; peak RSS <= baseline * 2 + 16384 KiB",
                  "scope": "single-run smoke guard; no optimization proof", "metrics": {}, "failures": []}
    for name in ("test-default", "test-all-features"):
        before = {row["target"]: row for row in baseline["test_totals"][name]["targets"]}
        after = {row["target"]: row for row in current["test_totals"][name]["targets"]}
        require(set(before) <= set(after), "baseline Cargo targets disappeared: " + name)
        for target, old in before.items():
            new = after[target]
            if new["passed"] < old["passed"]:
                comparison["failures"].append(name + ": passing test count decreased in " + target)
            old_ignored = {entry["name"] for entry in old["ignored_tests"]}
            new_ignored = {entry["name"] for entry in new["ignored_tests"]}
            if not new_ignored <= old_ignored:
                comparison["failures"].append(name + ": newly ignored tests in " + target)
        for target in set(after) - set(before):
            if after[target]["ignored"]:
                comparison["failures"].append(name + ": newly ignored tests in new target " + target)
    for name in TIMINGS:
        before, after = baseline["metrics"][name], current["metrics"][name]
        threshold = before * 2 + 100
        comparison["metrics"][name] = {"baseline": before, "current": after, "limit_ms": threshold}
        if after > threshold:
            comparison["failures"].append(name + " exceeded the single-run smoke limit")
    before, after = baseline["metrics"]["child_peak_rss_kib"], current["metrics"]["child_peak_rss_kib"]
    threshold = before * 2 + 16384
    comparison["metrics"]["child_peak_rss_kib"] = {"baseline": before, "current": after, "limit_kib": threshold}
    if after > threshold:
        comparison["failures"].append("child_peak_rss_kib exceeded the single-run smoke limit")
    return comparison


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--baseline", type=Path)
    args = parser.parse_args()
    assessment = assess(args.report)
    if args.baseline and assessment["status"] == "passed":
        try:
            comparison = compare(assessment, assess(args.baseline))
            assessment["comparison"] = comparison
            if comparison["failures"]:
                assessment["status"] = "failed"
                assessment["complete"] = False
                assessment["diagnostics"].extend(comparison["failures"])
        except (EvidenceError, OSError, TypeError, KeyError, ValueError) as error:
            assessment["status"] = "infrastructure_failed"
            assessment["complete"] = False
            assessment["diagnostics"].append("Baseline rejected: " + str(error))
    output = args.report.resolve().parent / "evaluation.json"
    try:
        temporary = output.with_suffix(".json.pending")
        temporary.write_text(json.dumps(assessment, indent=2) + "\n")
        temporary.replace(output)
    except OSError as error:
        print("evaluation evidence unavailable: " + str(error), file=sys.stderr)
        return 2
    print(json.dumps({"status": assessment["status"], "evaluation": str(output),
                      "diagnostics": assessment["diagnostics"]}), flush=True)
    return {"passed": 0, "failed": 1, "infrastructure_failed": 2}[assessment["status"]]


if __name__ == "__main__":
    raise SystemExit(main())
