#!/usr/bin/env python3
"""Verify unavailable-tool correction against a disposable actual TUI.

The fake provider deliberately returns run_command in Ask mode. Its command
would create a private sentinel if incorrectly executed. Ordinary Agent mode
uses a harmless command whose output must return to the provider. No live user
profile, secret, session or installed executable is modified.
"""
import argparse
import datetime
import json
from pathlib import Path
import tempfile

from compaction_pty import Terminal, binary_digest
from orc258_pty import FakeProvider, wait
from recovery_branch_pty import sessions


UNAVAILABLE_FOOTER = "Provider attempted unavailable tool: run_command"
REJECTED_RESULT = "this tool was not advertised for the current turn"
FORBIDDEN_COMMAND = "printf forbidden > unavailable-tool-must-not-exist.txt"
EMPTY_CATALOG = "Advertised tools for this request: []"
NO_TOOLS_GUIDANCE = "No tools were advertised for this request. Continue with a plain-text answer; do not attempt any tool call."


class CorrectiveResponses(dict):
    """A provider corrects itself only after receiving the host's exact catalog."""
    def __init__(self, provider):
        super().__init__()
        self.provider = provider
        self.conditions = {}

    def get(self, key, default=None):
        if key not in self.conditions:
            return super().get(key, default)
        reply, repeated = self.conditions[key]
        output = next((result["content"] for result in self.provider.requests[-1]["tool_outputs"] if result["id"] == key), "")
        if EMPTY_CATALOG in output and NO_TOOLS_GUIDANCE in output:
            return {"text": reply}
        return {"calls": [(repeated, "run_command", {"command": FORBIDDEN_COMMAND, "background": False})]}


def arm_run(provider, prompt, call_id, command):
    provider.arm(prompt, calls=[(call_id, "run_command", {"command": command, "background": False})])


def saved_records(terminal):
    terminal.command("/save")
    terminal.settle(0.4)
    return sessions(terminal)


def user_count(records, prompt):
    return sum(message["role"].lower() == "user" and message["content"] == prompt
        for record in records for message in record["messages"])


def assert_no_execution(terminal):
    assert not (terminal.workspace / "unavailable-tool-must-not-exist.txt").exists(), "unadvertised run_command executed"


def ask(terminal):
    terminal.command("/ask", "Ask mode", 5)


def switch_agent(terminal):
    # Empty-composer Tab is the existing real mode selector: Ask→Debug→Agent.
    terminal.write(b"\t")
    terminal.wait_text("Debug mode", 5)
    terminal.write(b"\t")
    terminal.wait_text("Agent mode", 5)
    terminal.settle()


def agent_case(terminal, provider):
    prompt, call_id, output = "UNAVAILABLE_AGENT_BASELINE", "available-agent-run", "UNAVAILABLE_AGENT_OUTPUT"
    arm_run(provider, prompt, call_id, "printf " + output)
    start = len(provider.requests)
    terminal.command(prompt, "ORC258_COMPLETE " + call_id, 15)
    terminal.settle(0.3)
    requests = provider.requests[start:]
    assert len(requests) == 2, requests
    assert all("run_command" in request["tools"] for request in requests), "Agent omitted run_command"
    assert any(result["id"] == call_id and output in result["content"] for result in requests[1]["tool_outputs"])
    records = saved_records(terminal)
    assert user_count(records, prompt) == 1
    assert any(call.get("id") == call_id and call["status"].lower() == "done" and output in call.get("output", "")
        for record in records for message in record["messages"] for call in message.get("tool_calls") or [])
    assert_no_execution(terminal)
    return {"agent_advertises_run_command": True, "advertised_command_output_returned_and_saved": True}


def single_correction_case(terminal, provider):
    ask(terminal)
    prompt, call_id = "UNAVAILABLE_ASK_SINGLE", "forbidden-ask-single"
    arm_run(provider, prompt, call_id, FORBIDDEN_COMMAND)
    provider.after_tools.conditions[call_id] = ("ORC258_COMPLETE " + call_id, call_id + "-repeated")
    start = len(provider.requests)
    terminal.command(prompt, "ORC258_COMPLETE " + call_id, 15)
    terminal.settle(0.3)
    requests = provider.requests[start:]
    assert len(requests) == 2, "expected exactly one corrective provider continuation"
    assert all(request["tools"] == [] for request in requests), "Ask exposed model-visible tools"
    assert requests[0]["user"] == requests[1]["user"] == prompt
    outputs = requests[1]["tool_outputs"]
    assert len(outputs) == 1 and outputs[0]["id"] == call_id and REJECTED_RESULT in outputs[0]["content"]
    assert EMPTY_CATALOG in outputs[0]["content"] and NO_TOOLS_GUIDANCE in outputs[0]["content"]
    assert_no_execution(terminal)
    records = saved_records(terminal)
    assert user_count(records, prompt) == 1
    assert any(call.get("id") == call_id and call["status"].lower() == "error" and REJECTED_RESULT in call.get("output", "")
        for record in records for message in record["messages"] for call in message.get("tool_calls") or [])
    return {"ask_schema_stays_empty": True, "first_unavailable_call_gets_resolved_error": True,
        "one_correction_finishes_original_request": True, "forbidden_command_never_executed": True,
        "rejected_tool_result_saved": True}


def repeated_case(terminal, provider):
    ask(terminal)
    terminal.resize(60, 30)
    terminal.settle()
    prompt, first, repeated = "UNAVAILABLE_ASK_REPEATED", "forbidden-ask-first", "forbidden-ask-repeated"
    arm_run(provider, prompt, first, FORBIDDEN_COMMAND)
    provider.arm_after(first, [(repeated, "run_command", {"command": FORBIDDEN_COMMAND, "background": False})])
    start = len(provider.requests)
    terminal.command(prompt, "Ask has no tools.", 15)
    terminal.settle(0.4)
    requests = provider.requests[start:]
    assert len(requests) == 2, "repeated unavailable call exceeded one correction"
    assert all(request["tools"] == [] for request in requests)
    assert any(result["id"] == first and REJECTED_RESULT in result["content"] for result in requests[1]["tool_outputs"])
    assert_no_execution(terminal)
    failure_screen = terminal.screen.text()
    assert "Ask has no tools" in failure_screen and "Tab twice to Agent" in failure_screen and "resend" in failure_screen
    records = saved_records(terminal)
    assert user_count(records, prompt) == 1
    reply = "UNAVAILABLE_USER_RETRY_COMPLETE"
    provider.arm(prompt, text=reply)
    with provider.lock:
        provider.after_tools[first] = {"text": reply}
        provider.after_tools[repeated] = {"text": reply}
    retry_start = len(provider.requests)
    terminal.command("/retry", reply, 20)
    terminal.settle(0.4)
    retried = provider.requests[retry_start:]
    assert len(retried) == 1 and retried[0]["user"] == prompt and retried[0]["tools"] == []
    assert_no_execution(terminal)
    records = saved_records(terminal)
    assert any(message["content"] == reply for record in records for message in record["messages"])
    assert all(sum(message["role"].lower() == "user" and message["content"] == prompt for message in record["messages"]) <= 1
        for record in records), "retry duplicated original user prompt"
    switch_agent(terminal)
    agent_prompt, agent_call, agent_output = "UNAVAILABLE_SWITCH_AGENT", "available-after-mode-switch", "UNAVAILABLE_MODE_SWITCH_OUTPUT"
    arm_run(provider, agent_prompt, agent_call, "printf " + agent_output)
    agent_start = len(provider.requests)
    terminal.command(agent_prompt, "ORC258_COMPLETE " + agent_call, 15)
    terminal.settle(0.3)
    enabled = provider.requests[agent_start:]
    assert len(enabled) == 2 and all("run_command" in request["tools"] for request in enabled)
    assert any(result["id"] == agent_call and agent_output in result["content"] for result in enabled[1]["tool_outputs"])
    assert_no_execution(terminal)
    return {"repeated_call_stops_after_one_correction": True, "truthful_unavailable_footer": True,
        "narrow_60_column_guidance_visible": True,
        "user_retry_preserves_original_request_and_empty_schema": True,
        "mode_switch_enables_advertised_command_on_new_turn": True,
        "forbidden_command_never_executed": True, "failure_screen": failure_screen}


def reproduce_case(terminal, provider):
    ask(terminal)
    prompt = "UNAVAILABLE_OLD_SCREENSHOT"
    arm_run(provider, prompt, "forbidden-before-fix", FORBIDDEN_COMMAND)
    provider.after_tools.conditions["forbidden-before-fix"] = ("CORRECTED_HOST_CATALOG_COMPLETE", "forbidden-before-fix-repeated")
    start = len(provider.requests)
    terminal.command(prompt, UNAVAILABLE_FOOTER, 10)
    terminal.settle(0.3)
    requests = provider.requests[start:]
    assert len(requests) == 2 and all(request["tools"] == [] for request in requests)
    assert_no_execution(terminal)
    return {"exact_screenshot_footer_reproduced": True, "ask_tools_empty": True,
        "repeated_unavailable_call_stopped_after_one_correction": True, "forbidden_command_never_executed": True,
        "failure_screen": terminal.screen.text()}


def plan_transition_case(terminal, provider):
    terminal.command("/plan", "Plan mode", 5)
    terminal.resize(60, 30)
    terminal.settle()
    prompt, first = "UNAVAILABLE_PLAN_REPEATED", "forbidden-plan-first"
    arm_run(provider, prompt, first, FORBIDDEN_COMMAND)
    provider.arm_after(first, [("forbidden-plan-repeated", "run_command", {
        "command": FORBIDDEN_COMMAND, "background": False})])
    start = len(provider.requests)
    terminal.command(prompt, "Plan: Empty input", 15)
    terminal.settle(0.3)
    plan_requests = provider.requests[start:]
    assert len(plan_requests) == 2 and all("run_command" not in request["tools"] for request in plan_requests)
    assert plan_requests[0]["tools"], "Plan unexpectedly advertised no read-only tools"
    rejection = next(result["content"] for result in plan_requests[1]["tool_outputs"] if result["id"] == first)
    catalog = rejection.split("Advertised tools for this request: ", 1)[1].split("\n", 1)[0]
    assert json.loads(catalog) == sorted(plan_requests[0]["tools"]), "correction did not list the exact advertised catalog"
    failure_screen = terminal.screen.text()
    assert "Tab until Agent" in failure_screen and "resend" in failure_screen
    assert_no_execution(terminal)
    for mode in ("Ask", "Debug"):
        terminal.write(b"\t")
        terminal.wait_text(mode + " mode", 5)
        marker, reply = "UNAVAILABLE_PLAN_TO_" + mode.upper(), "MODE_PROBE_" + mode.upper() + "_COMPLETE"
        provider.arm(marker, text=reply)
        probe_start = len(provider.requests)
        terminal.command(marker, reply, 10)
        terminal.settle(0.2)
        probe = provider.requests[probe_start:]
        assert len(probe) == 1
        assert probe[0]["tools"] == [] if mode == "Ask" else "run_command" in probe[0]["tools"]
        assert_no_execution(terminal)
    terminal.write(b"\t")
    terminal.wait_text("Agent mode", 5)
    agent_checks = agent_case(terminal, provider)
    return {"plan_excludes_mutating_command": True, "correction_lists_exact_advertised_catalog": True,
        "narrow_plan_resend_guidance_visible": True, "actual_plan_ask_debug_agent_transition": True,
        "forbidden_command_never_executed": True, **agent_checks, "failure_screen": failure_screen}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--receipt", type=Path)
    parser.add_argument("--reproduce", action="store_true")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix="ca-unavailable-tool-pty-", dir="/private/tmp")).resolve()
    root.chmod(0o700)
    receipt = {"status": "running", "root": str(root), "binary": str(binary), "binary_sha256": binary_digest(binary),
        "mode": "reproduce_before_fix" if args.reproduce else "verify_fix", "cases": {},
        "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    cases = {"reproduction": reproduce_case} if args.reproduce else {
        "agent": agent_case, "ask-correction": single_correction_case, "ask-repeated-retry-agent-switch": repeated_case,
        "plan-ask-debug-agent-transition": plan_transition_case}
    for name, case in cases.items():
        case_root = root / name
        case_root.mkdir(mode=0o700)
        provider = FakeProvider()
        provider.protocol = "chat_completions"
        provider.after_tools = CorrectiveResponses(provider)
        terminal = None
        result = {"status": "running"}
        try:
            terminal = Terminal(binary, case_root, provider)
            result["checks"] = case(terminal, provider)
            result["status"] = "reproduced" if args.reproduce else "passed"
        except Exception as error:
            result["status"] = "failed"
            result["error"] = str(error)
            result["screen"] = terminal.screen.text() if terminal else "startup failed"
        finally:
            if terminal:
                result["output_bytes"] = terminal.bytes
                terminal.close()
                result["exit_code"] = terminal.process.returncode
                result["terminal_restored"] = terminal.restored_termios
                if terminal.process.returncode or not terminal.restored_termios:
                    result["status"] = "failed"
                    result.setdefault("error", "terminal did not exit successfully and restore termios")
            provider.close()
            result["provider_request_receipts"] = provider.requests
            receipt["cases"][name] = result
    expected = "reproduced" if args.reproduce else "passed"
    receipt["status"] = expected if all(case["status"] == expected for case in receipt["cases"].values()) else "failed"
    receipt["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    output = root / "result.json"
    output.write_text(json.dumps(receipt, indent=2) + "\n")
    if args.receipt:
        args.receipt.resolve().write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"status": receipt["status"], "root": str(root), "result": str(output),
        "binary_sha256": receipt["binary_sha256"], "cases": {name: {key: (value[:600] if key == "error" else value)
            for key, value in result.items() if key not in ("screen", "provider_request_receipts", "checks")}
            for name, result in receipt["cases"].items()}}), flush=True)
    return 0 if receipt["status"] == expected else 1


if __name__ == "__main__":
    raise SystemExit(main())
