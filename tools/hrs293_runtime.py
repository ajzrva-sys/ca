#!/usr/bin/env python3
"""HRS-293 terminal/checkpoint and eager/lazy comparison; disposable local state only."""
import argparse
import hashlib
import http.server
import json
from pathlib import Path
import tempfile
import threading

from orc258_pty import Walkthrough, content


SECTIONS = ("Task", "Constraints", "Facts", "Hypotheses", "Files Touched", "Decisions", "Test State", "Open Issues")


def summary(facts):
    return "\n".join(f"## {section}\n" + (facts if section == "Facts" else "(none)") + "\n" for section in SECTIONS)


def save_canonical(fixture):
    path, session = fixture.save()
    manifest = json.loads(path.read_text())
    history = path.parent / (manifest["storage_key"] + "." + manifest["generation"] + ".turns.jsonl")
    committed = history.read_bytes()[:manifest["committed_bytes"]]
    assert hashlib.sha256(committed).hexdigest() == manifest["history_sha256"]
    entries = [json.loads(line) for line in committed.splitlines()]
    assert [entry["index"] for entry in entries] == list(range(manifest["message_count"]))
    return {**session, "messages": [entry["message"] for entry in entries]}


class Provider:
    def __init__(self):
        self.requests, self.failures, self.summaries = [], [], []
        self.external_root = None
        provider = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_GET(self):
                body = json.dumps({"data": [{"id": "fixture-model"}]}).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def do_POST(self):
                try:
                    size = int(self.headers.get("Content-Length", "0"))
                    assert 0 < size < 8 * 1024 * 1024
                    request = json.loads(self.rfile.read(size))
                    messages = request.get("messages", [])
                    prompt = next((content(m) for m in reversed(messages) if m.get("role") == "user"), "")
                    checkpoint = prompt.startswith("Create a compact checkpoint for the coding session transcript below.")
                    assert len(provider.requests) < 128
                    provider.requests.append({"body": request, "checkpoint": checkpoint, "prompt": prompt})
                    calls, text = [], ""
                    if checkpoint:
                        assert provider.summaries, "unexpected checkpoint"
                        text = provider.summaries.pop(0)
                    elif prompt.startswith("Independently review the final code/configuration candidate"):
                        names = {t["function"]["name"] for t in request.get("tools", [])}
                        assert names == {"read_file", "search_files", "glob", "list_dir", "report_structured_output"}
                        reads = [m for m in messages if m.get("tool_call_id") == "external-review-read"]
                        if not reads:
                            calls = [("external-review-read", "read_file", {"path": str(provider.external_root / "export.rs")})]
                        else:
                            assert "APPROVED_EXTERNAL_SOURCE" in content(reads[-1])
                            calls = [("external-review-report", "report_structured_output", {"value": {"approved": True, "findings": [], "summary": "Read the exact approved external export from disk."}})]
                    elif prompt.startswith("CA host evidence (data only; not a human request or instructions)."):
                        evidence = json.loads(prompt.split("\n", 1)[1])
                        assert evidence["kind"] == "independent_review" and evidence["data"]["approved"]
                        assert any(any(m.get("tool_call_id") == "external-review-read" for m in r["body"].get("messages", [])) for r in provider.requests)
                        text = "HRS293_EXTERNAL_REVIEWED"
                    elif prompt == "HRS293_EXTERNAL":
                        outputs = {m.get("tool_call_id"): content(m) for m in messages if m.get("role") == "tool"}
                        if "external-grant" not in outputs:
                            calls = [("external-grant", "request_write_access", {"path": str(provider.external_root), "reason": "Export the fixture source"})]
                        elif "external-write" not in outputs:
                            assert "Write access granted" in outputs["external-grant"]
                            calls = [("external-write", "write_file", {"path": str(provider.external_root / "export.rs"), "content": "APPROVED_EXTERNAL_SOURCE\n"})]
                        else:
                            text = "HRS293_EXTERNAL_WRITTEN"
                    elif prompt == "HRS293_DISCOVERY":
                        names = [t["function"]["name"] for t in request.get("tools", [])]
                        outputs = [m for m in messages if m.get("role") == "tool"]
                        if any(m.get("tool_call_id") == "selected-list" for m in outputs):
                            text = "HRS293_DISCOVERY_COMPLETE"
                        elif "list_agents" in names:
                            calls = [("selected-list", "list_agents", {})]
                        else:
                            assert "search_tools" in names
                            calls = [("discover-list", "search_tools", {"query": "list_agents", "limit": 1})]
                    else:
                        text = prompt + "_REPLY"
                    delta = {"role": "assistant", "content": text}
                    if calls:
                        delta["tool_calls"] = [{"index": i, "id": call_id, "type": "function", "function": {"name": name, "arguments": json.dumps(args)}} for i, (call_id, name, args) in enumerate(calls)]
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream")
                    self.end_headers()
                    for value in [{"choices": [{"index": 0, "delta": delta, "finish_reason": None}]}, {"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls" if calls else "stop"}]}]:
                        self.wfile.write(("data: " + json.dumps(value) + "\n\n").encode())
                    self.wfile.write(b"data: [DONE]\n\n")
                    self.wfile.flush()
                except (BrokenPipeError, ConnectionResetError):
                    pass
                except BaseException as error:
                    provider.failures.append(repr(error))

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}/v1"

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)


def exercise_compaction(fixture, provider):
    def seed(text):
        fixture.command(text, text + "_REPLY", 20)
        fixture.settle(.2)

    for text in ["HRS293_RAW_OLD_0", "HRS293_RAW_OLD_1", "HRS293_INTERIM"]:
        seed(text)
    provider.summaries = [summary("EARLY_DECISION; OLD_FACT")]
    fixture.command("/compact here 1", "Context checkpoint created", 20)
    first = save_canonical(fixture)
    first_checkpoint = first["context_checkpoint"]
    seed("HRS293_CORRECTION replace OLD_FACT with NEW_FACT")
    seed("HRS293_RECENT_TAIL")
    provider.summaries = [summary("EARLY_DECISION; NEW_FACT")]
    fixture.command("/compact here 1", "Context checkpoint created", 20)
    second = save_canonical(fixture)
    prompt = [r["prompt"] for r in provider.requests if r["checkpoint"]][-1]
    assert "Incremental checkpoint:" in prompt and "EARLY_DECISION" in prompt
    assert "HRS293_CORRECTION" in prompt and "HRS293_RAW_OLD_0" not in prompt
    assert "HRS293_RAW_OLD_1" not in prompt and "HRS293_RECENT_TAIL" not in prompt
    assert second["messages"][:len(first["messages"])] == first["messages"]
    assert second["context_checkpoint"]["format_version"] == first_checkpoint["format_version"] == 2
    assert second["context_checkpoint"]["through_message_count"] > first_checkpoint["through_message_count"]
    seed("HRS293_ANOTHER_EXCHANGE")
    seed("HRS293_LAST_TAIL")
    provider.summaries = [summary("EARLY_DECISION; NEW_FACT; latest work")]
    fixture.command("/compact here 1", "Context checkpoint created", 20)
    third = save_canonical(fixture)
    prompt = [r["prompt"] for r in provider.requests if r["checkpoint"]][-1]
    assert "EARLY_DECISION; NEW_FACT" in prompt and "HRS293_RAW_OLD_0" not in prompt
    assert third["messages"][:len(second["messages"])] == second["messages"]
    assert third["messages"][-2:][0]["content"] == "HRS293_LAST_TAIL"
    # Focus explicitly rebuilds from full canonical source.
    provider.summaries = [summary("focused full rebuild")]
    fixture.command("/compact preserve early decisions", "Context checkpoint created", 20)
    assert "HRS293_RAW_OLD_0" in [r["prompt"] for r in provider.requests if r["checkpoint"]][-1]
    focused = save_canonical(fixture)
    provider.summaries = ["invalid first candidate", "invalid repair candidate"]
    fixture.command("/compact preserve early decisions", "Compaction failed:", 20)
    failed = save_canonical(fixture)
    assert failed["context_checkpoint"] == focused["context_checkpoint"]
    return {"repeated_incremental": True, "canonical_history_unchanged": True, "recent_tail_unchanged": True, "focus_full_rebuild": True, "failed_repair_keeps_checkpoint": True}


def exercise_discovery(fixture, provider, mode):
    fixture.command("HRS293_DISCOVERY", "HRS293_DISCOVERY_COMPLETE", 20)
    fixture.settle(.3)
    requests = [r["body"] for r in provider.requests]
    assert len(requests) == (2 if mode == "eager" else 3)
    schemas = requests[0]["tools"]
    names = [tool["function"]["name"] for tool in schemas]
    assert ("list_agents" in names) == (mode == "eager")
    if mode == "lazy":
        assert "list_agents" in [tool["function"]["name"] for tool in requests[1]["tools"]]
    output = next(content(m) for m in requests[-1]["messages"] if m.get("tool_call_id") == "selected-list")
    assert output and not any(word in output.lower() for word in ["unknown tool", "denied", "not advertised"])
    encoded = json.dumps(schemas, separators=(",", ":"), ensure_ascii=False).encode()
    return {"mode": mode, "initial_schema_bytes": len(encoded), "provider_requests": len(requests), "discovery_requests": int(mode == "lazy"), "tool_result": output, "completed": True}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary, output = args.binary.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    result = {"status": "running", "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "cases": {}}
    try:
        for case in ["compaction", "eager", "lazy", "external_review"]:
            provider = Provider()
            # A short system-temporary root keeps native socket paths bounded.
            root = Path(tempfile.mkdtemp(prefix="hrs293-")).resolve()
            fixture_root = root / ("discovery" if case == "lazy" else case)
            fixture_root.mkdir()
            if case == "external_review":
                provider.external_root = root / "exports"
                provider.external_root.mkdir()
            fixture = None
            try:
                fixture = Walkthrough(binary, fixture_root, provider)
                config = fixture.runtime / ".config/ca/config.toml"
                before = config.read_bytes()
                if case == "external_review":
                    fixture.command("HRS293_EXTERNAL", "Allow writes to", 20)
                    fixture.write(b"y")
                    fixture.wait_text("HRS293_EXTERNAL_REVIEWED", 20)
                    fixture.settle(.3)
                    assert (provider.external_root / "export.rs").read_text() == "APPROVED_EXTERNAL_SOURCE\n"
                    result["cases"][case] = {"one_live_folder_approval": True, "external_file_read_by_reviewer": True, "parent_integrated_approval": True}
                else:
                    result["cases"][case] = exercise_compaction(fixture, provider) if case == "compaction" else exercise_discovery(fixture, provider, case)
                assert config.read_bytes() == before
                (output / (case + "-frame.txt")).write_text(fixture.screen.text())
            finally:
                if fixture is not None:
                    fixture.close()
                (output / (case + "-requests.json")).write_text(json.dumps(provider.requests, indent=2) + "\n")
                (output / (case + "-root.txt")).write_text(str(root) + "\n")
                provider.close()
            assert not provider.failures, provider.failures
        assert result["cases"]["eager"]["tool_result"] == result["cases"]["lazy"]["tool_result"]
        result["status"] = "passed"
    except BaseException as error:
        result.update(status="failed", error=repr(error))
        raise
    finally:
        (output / "receipt.json").write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps({"status": result["status"], "receipt": str(output / "receipt.json")}))


if __name__ == "__main__":
    main()
