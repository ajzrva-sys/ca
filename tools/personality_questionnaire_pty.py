#!/usr/bin/env python3
"""Exercise the interactive personality questionnaire using real terminal keys.

python3 tools/personality_questionnaire_pty.py --binary target/release/ca --receipt scratch/questionnaire/interactive-release.json
All homes, profile files and sessions are disposable. The native CA case uses
only a captured loopback fake provider; questionnaire interaction must never
become a provider request. This is bounded keyboard/frame evidence, not a claim
of whole-app coverage or paid-model personality effectiveness.
"""
import argparse
import base64
import datetime
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import struct
import subprocess
import tempfile
import termios
import time

from bug257_pty import Fixture, Screen
from compaction_pty import Terminal, binary_digest
from questionnaire_pty import Provider


CUSTOM = "PERSONALITY_INTERACTIVE_CUSTOM: Give one concrete example in every explanation."
EXTRA = "PERSONALITY_INTERACTIVE_ADDITIONAL: Prefer exact names over vague descriptions."
IMPORTED = "PERSONALITY_IMPORTED_MANUAL: Preserve this existing steering."
NEXT_PROMPT = "PERSONALITY_INTERACTIVE_NEXT_REQUEST"
NEXT_REPLY = "PERSONALITY_INTERACTIVE_NEXT_COMPLETE"


class QuestionnaireTerminal(Fixture):
    """A standalone, provider-free actual terminal with a reusable private home."""
    def __init__(self, binary, root, *, profile="default", width=80, height=24, default=False):
        self.binary, self.root = binary, root
        self.workspace = root / "workspace"
        self.workspace.mkdir(exist_ok=True)
        self.runtime = root / "runtime"
        self.runtime.mkdir(exist_ok=True)
        self.environment = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(self.runtime),
            "TERM": "xterm-256color", "LANG": "en_US.UTF-8", "COLORFGBG": "15;0"}
        self.screen = Screen(width, height)
        self.bytes, self.raw_tail = 0, b""
        self.master, slave = pty.openpty()
        self.original_termios = termios.tcgetattr(slave)
        self.restored_termios = False
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))

        def terminal_session():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        args = [str(binary), "harness", "questionnaire", "--profile", profile]
        if not default:
            args.append("setup")
        self.process = subprocess.Popen(args, cwd=self.workspace, stdin=slave, stdout=slave,
            stderr=slave, env=self.environment, preexec_fn=terminal_session)
        os.close(slave)

    def settle(self, seconds=0.15):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            assert self.read(0.02), f"questionnaire exited {self.process.returncode}: {self.screen.text()}"

    def wait_exit(self, timeout=5):
        deadline = time.monotonic() + timeout
        while self.process.poll() is None:
            self.read(0.02)
            assert time.monotonic() < deadline, "questionnaire did not exit promptly"
        self.read(0)
        return self.process.returncode

    def close(self):
        if getattr(self, "closed", False):
            return
        if self.process.poll() is None:
            self.write(b"\x03")
            deadline = time.monotonic() + 5
            while self.process.poll() is None and time.monotonic() < deadline:
                self.read(0.02)
        if self.process.poll() is None:
            self.process.terminate()
        self.process.wait(timeout=5)
        self.read(0)
        self.restored_termios = termios.tcgetattr(self.master) == self.original_termios
        os.close(self.master)
        self.closed = True


def frame(terminal, label, frames):
    terminal.settle()
    frames.append({"label": label, "width": terminal.screen.width,
        "height": terminal.screen.height, "text": terminal.screen.text()})


def key(terminal, value, expect=None):
    terminal.write(value)
    if expect:
        terminal.wait_text(expect, 5)
    else:
        terminal.settle()


def exit_key(terminal, value=b"\x03"):
    terminal.write(value)
    terminal.wait_exit()


def personality_path(root, profile="default"):
    config = root / "runtime/.config/ca"
    return config / "personality.md" if profile == "default" else config / "profiles" / profile / "personality.md"


def metadata(path):
    text = path.read_text()
    match = re.search(r"<!-- ca-personality-preferences:v1\n([A-Za-z0-9+/]+)\n-->", text)
    assert match, "saved personality lacks questionnaire metadata"
    encoded = match.group(1)
    value = json.loads(base64.b64decode(encoded + "=" * (-len(encoded) % 4)))
    assert value["version"] == 1, "unknown saved preference schema"
    active = re.sub(r"<!--.*?-->", "", text, flags=re.S)
    assert stat_private(path), "personality file is not private"
    return text, active, value


def stat_private(path):
    return path.stat().st_mode & 0o777 == 0o600


def run_cli(binary, root, *args):
    runtime = root / "runtime"
    runtime.mkdir(exist_ok=True)
    environment = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(runtime),
        "TERM": "xterm-256color", "LANG": "en_US.UTF-8"}
    result = subprocess.run([str(binary), "harness", "questionnaire", *args], cwd=root,
        env=environment, capture_output=True, text=True, timeout=10)
    return {"args": list(args), "exit_code": result.returncode, "stdout": result.stdout, "stderr": result.stderr}


def baseline_case(binary, root):
    terminal = QuestionnaireTerminal(binary, root)
    result = {"status": "running"}
    try:
        terminal.wait_text("Quick setup", 3)
        result["status"] = "passed"
    except Exception as error:
        result["status"] = "failed"
        result["error"] = str(error)
        result["screen"] = terminal.screen.text()
    finally:
        terminal.close()
        result["output_bytes"] = terminal.bytes
        result["exit_code"] = terminal.process.returncode
        result["terminal_restored"] = terminal.restored_termios
    return result


class Case:
    def __init__(self, binary, root):
        self.binary, self.root = binary, root
        self.frames, self.terminals, self.providers = [], [], []

    def terminal(self, *, root=None, **kwargs):
        terminal = QuestionnaireTerminal(self.binary, self.root if root is None else root, **kwargs)
        self.terminals.append((terminal, {"expected_exit_code": 0}))
        return terminal

    def native(self, *, root=None):
        provider = Provider()
        self.providers.append(provider)
        terminal = Terminal(self.binary, self.root if root is None else root, provider)
        terminal.resize(80, 24)
        terminal.settle()
        self.terminals.append((terminal, {"expected_exit_code": 0}))
        return terminal, provider

    def capture(self, terminal, label):
        frame(terminal, label, self.frames)

    def finish(self, terminal):
        record = next(record for owned, record in self.terminals if owned is terminal)
        if record.get("closed"):
            return
        if not isinstance(terminal, QuestionnaireTerminal) and terminal.process.poll() is None:
            # First close a possibly still-open native editor; close() then
            # exits the owned idle CA process instead of terminating a modal.
            terminal.write(b"\x03")
            terminal.read(0.1)
        terminal.close()
        record.update({"closed": True, "exit_code": terminal.process.returncode,
            "terminal_restored": terminal.restored_termios, "output_bytes": terminal.bytes})


def overview(terminal):
    terminal.wait_text("How would you like CA", 5)
    assert "Quick setup" in terminal.screen.text()


def quick(terminal):
    overview(terminal)
    key(terminal, b"1\r")


def custom(terminal, text):
    key(terminal, b"4\r", "Ctrl-S Use answer")
    key(terminal, b"\x1b[200~" + text.encode() + b"\x1b[201~")
    assert text.split(":", 1)[0] in terminal.screen.text(), "custom answer was not visibly editable"
    key(terminal, b"\x13")


def review(terminal):
    key(terminal, b"\x1b", "How would you like CA")
    key(terminal, b"4\r", "Review and save")
    terminal.wait_text("Enter saves", 5)


def save(terminal):
    key(terminal, b"\r", "Preferences saved")


def setup_edit_and_resume(case):
    path = personality_path(case.root)
    path.parent.mkdir(parents=True, mode=0o700)
    path.write_text("# Existing personality\n\n" + IMPORTED + "\n")
    path.chmod(0o600)
    original = path.read_bytes()
    terminal = case.terminal()
    overview(terminal)
    case.capture(terminal, "80x24 overview")
    quick(terminal)
    key(terminal, b"2\r")
    key(terminal, b"\x1b[D")
    case.capture(terminal, "previous answer restored for editing")
    key(terminal, b"3\r")
    custom(terminal, CUSTOM)
    for _ in range(6):
        key(terminal, b"5\r")
    terminal.wait_text("Review and save", 5)
    case.capture(terminal, "80x24 completed quick setup review")
    save(terminal)
    case.capture(terminal, "80x24 explicit save confirmation")
    text, active, value = metadata(path)
    assert value["answers"]["voice.tone"] == {"Choice": "precise"}, value
    assert value["answers"]["detail.final"] == {"Custom": CUSTOM}, value
    assert "Use a precise, professional voice." in active, active
    assert CUSTOM in active and IMPORTED in active, "generated steering lost an answer or imported instructions"
    assert "Warm and collaborative" not in active, "Back/edit left the superseded choice active"
    assert "ca-personality-preferences:v1" not in active, "metadata became active steering"
    backups = list(path.parent.glob(".personality-recovery-*.md"))
    assert len(backups) == 1 and backups[0].read_bytes() == original and stat_private(backups[0]), "saving did not preserve a private recovery copy"
    exit_key(terminal)
    case.finish(terminal)

    reopened = case.terminal(width=60, height=20, default=True)
    quick(reopened)
    reopened.wait_text("3 of 8", 5)
    case.capture(reopened, "60x20 resume at first unanswered question")
    key(reopened, b"\x1b[D\x1b[D")
    case.capture(reopened, "60x20 reopened saved first answer")
    assert "Precise and formal (current)" in reopened.screen.text(), "saved choice did not reopen visibly"
    key(reopened, b"\r")
    case.capture(reopened, "60x20 reopened custom answer")
    assert "Custom" in reopened.screen.text(), "saved custom answer not represented in the question"
    key(reopened, b"4\r", "Ctrl-S Use answer")
    assert "PERSONALITY_INTERACTIVE_CUSTOM" in reopened.screen.text(), "saved custom text was not restored"
    key(reopened, b"\x1b")
    key(reopened, b"\x1b", "How would you like CA")
    exit_key(reopened)
    case.finish(reopened)
    assert path.read_text() == text, "reopening/cancelling changed the saved file"
    return {"quick_setup_choices_and_custom_answer": True, "back_edit_replaces_previous_choice": True,
        "review_requires_explicit_save": True, "human_markdown_and_private_metadata_saved_atomically": True,
        "existing_manual_steering_preserved": True, "previous_file_preserved_in_private_recovery_copy": True,
        "resume_at_first_unanswered_question": True,
        "saved_answers_reopen_at_60x20": True,
        "reopen_cancel_changes_no_bytes": True}


def quick_scope_and_partial_save(case):
    """Quick setup is eight optional answers, and a partial draft can be saved."""
    terminal = case.terminal()
    quick(terminal)
    for question in range(1, 9):
        text = terminal.screen.text()
        assert f"Quick setup · {question} of 8" in text, "Quick setup left its eight-question scope"
        assert "/48 answered" not in text, "Quick setup displayed the full questionnaire's answer counter"
        key(terminal, b"3\r")
    terminal.wait_text("Enter saves", 5)
    case.capture(terminal, "quick setup ends after exactly eight questions")
    text = terminal.screen.text()
    assert "Quick setup · 8/8 answered" in text, "completed Quick setup lacks its own answer count"
    assert "Quick setup complete" in text and "optional" in text, "Quick setup implies more answers are required"
    assert "/48 answered" not in text, "completed Quick setup displayed the full questionnaire's answer counter"
    save(terminal)
    saved_text, _, saved_preferences = metadata(personality_path(case.root))
    assert len(saved_preferences["answers"]) == 8, "Quick setup saved more or fewer than eight answers"
    assert all(isinstance(answer, dict) and "Choice" in answer for answer in saved_preferences["answers"].values())
    exit_key(terminal)
    case.finish(terminal)

    reopened = case.terminal()
    quick(reopened)
    case.capture(reopened, "all eight quick answers reopen")
    for question in range(1, 9):
        text = reopened.screen.text()
        assert f"Quick setup · {question} of 8" in text
        assert "Quick setup · 8/8 answered" in text
        assert re.search(r"3 [^\n]+\(current\)", text), "a saved Quick setup choice did not reopen"
        key(reopened, b"3\r")
    reopened.wait_text("Enter saves", 5)
    exit_key(reopened)
    case.finish(reopened)
    assert personality_path(case.root).read_text() == saved_text, "reopening unchanged quick answers rewrote preferences"

    partial_root = case.root / "partial-quick"
    partial_root.mkdir(mode=0o700)
    partial = case.terminal(root=partial_root)
    quick(partial)
    key(partial, b"2\r")
    key(partial, b"3\r")
    partial.wait_text("3 of 8", 5)
    case.capture(partial, "partial quick setup advertises review and optional answers")
    text = partial.screen.text()
    assert "Ctrl-S Review" in text, "questions conceal the save-before-finishing shortcut"
    assert "Save at any point. Unanswered questions are optional." in text, "questions imply all answers are required"
    key(partial, b"\x13", "Enter saves")
    case.capture(partial, "two quick answers can be reviewed and saved")
    text = partial.screen.text()
    assert "Quick setup · 2/8 answered" in text, "partial Quick setup lacks its own answer count"
    assert "Review and save whenever you are ready." in text
    assert "/48 answered" not in text, "partial Quick setup displayed the full questionnaire's answer counter"
    save(partial)
    partial_saved_text, _, partial_preferences = metadata(personality_path(partial_root))
    assert len(partial_preferences["answers"]) == 2, "saving a partial draft added unanswered preferences"
    assert partial_preferences["answers"]["voice.tone"] == {"Choice": "warm"}
    assert partial_preferences["answers"]["detail.final"] == {"Choice": "thorough"}
    exit_key(partial)
    case.finish(partial)

    resumed = case.terminal(root=partial_root)
    quick(resumed)
    resumed.wait_text("3 of 8", 5)
    case.capture(resumed, "saved partial quick setup resumes at question three")
    assert "Quick setup · 2/8 answered" in resumed.screen.text()
    key(resumed, b"\x1b[D\x1b[D")
    assert "2 Warm and collaborative (current)" in resumed.screen.text()
    key(resumed, b"\r")
    assert "3 Thorough explanation (current)" in resumed.screen.text()
    exit_key(resumed)
    case.finish(resumed)
    assert personality_path(partial_root).read_text() == partial_saved_text, "reopening a partial draft changed saved preferences"

    native_root = case.root / "native-quick"
    native_root.mkdir(mode=0o700)
    native, provider = case.native(root=native_root)
    native_path = personality_path(native_root)
    native_path.write_text(partial_saved_text)
    native_path.chmod(0o600)
    native.command("/personality", "Quick setup", 5)
    quick(native)
    native.wait_text("3 of 8", 5)
    case.capture(native, "native quick setup resumes two saved answers")
    assert "Quick setup · 2/8 answered" in native.screen.text()
    for question in range(3, 9):
        text = native.screen.text()
        assert f"Quick setup · {question} of 8" in text, "native Quick setup left its eight-question scope"
        assert "/48 answered" not in text, "native Quick setup displayed the full questionnaire's answer counter"
        key(native, b"1\r")
    native.wait_text("Enter saves", 5)
    case.capture(native, "native quick setup ends at eight and makes remaining answers optional")
    text = native.screen.text()
    assert "Quick setup · 8/8 answered" in text
    assert "Quick setup complete" in text and "optional" in text
    assert "/48 answered" not in text
    save(native)
    _, _, native_preferences = metadata(native_path)
    assert len(native_preferences["answers"]) == 8, "native Quick setup saved answers outside its scope"
    for question_id, answer in partial_preferences["answers"].items():
        assert native_preferences["answers"][question_id] == answer, "native completion changed a previously saved answer"
    assert not provider.snapshot(), "native Quick setup or Save contacted the model"
    case.finish(native)
    assert not provider.snapshot(), "closing native Quick setup contacted the model"
    return {"quick_setup_stays_within_eight_questions": True,
        "quick_complete_explains_remaining_questions_are_optional": True,
        "quick_counters_exclude_full_48_question_total": True,
        "exactly_eight_answers_save_and_reopen": True,
        "question_footer_exposes_save_at_any_point": True,
        "ctrl_s_saves_two_answers_without_finishing": True,
        "partial_quick_resumes_at_first_unanswered_question": True,
        "reopening_changes_no_saved_bytes": True,
        "native_quick_resumes_and_stays_within_eight_questions": True,
        "native_quick_completion_marks_other_questions_optional": True,
        "native_quick_saves_exactly_eight_answers": True,
        "native_quick_makes_no_provider_requests": True}


def small_screen_cancel(case):
    terminal = case.terminal(width=60, height=20, default=True)
    overview(terminal)
    case.capture(terminal, "60x20 default command overview")
    quick(terminal)
    assert "Enter Next" in terminal.screen.text() and "Esc Menu" in terminal.screen.text(), "question controls are clipped at 60x20"
    custom(terminal, CUSTOM)
    terminal.resize(30, 10)
    terminal.wait_text("Make the terminal wider", 5)
    case.capture(terminal, "undersized terminal preserves draft")
    terminal.resize(60, 20)
    terminal.wait_text("2 of 8", 5)
    key(terminal, b"\x1b", "How would you like CA")
    key(terminal, b"6\r")
    case.capture(terminal, "60x20 discard confirmation")
    assert "Keep editing" in terminal.screen.text() and "Discard" in terminal.screen.text(), "dirty close skipped confirmation"
    exit_key(terminal, b"\x1b[B\r")
    case.finish(terminal)
    assert not personality_path(case.root).exists(), "discarding an unsaved questionnaire created steering"
    reopened = case.terminal(width=60, height=20)
    quick(reopened)
    case.capture(reopened, "60x20 reopening discarded answers")
    assert "PERSONALITY_INTERACTIVE_CUSTOM" not in reopened.screen.text(), "discarded answer survived reopen"
    exit_key(reopened)
    case.finish(reopened)
    assert not personality_path(case.root).exists(), "Ctrl-C cancellation created steering"
    return {"default_tty_command_is_interactive": True, "60x20_question_controls_visible": True,
        "resize_preserves_unsaved_draft": True, "dirty_close_requires_keep_or_discard": True,
        "discard_and_ctrl_c_do_not_create_a_file": True}


def full_walkthrough_sections_and_additional(case):
    terminal = case.terminal()
    overview(terminal)
    key(terminal, b"2\r", "1 of 48")
    case.capture(terminal, "full walkthrough begins with all 48 questions")
    key(terminal, b"1\r")
    for _ in range(46):
        key(terminal, b"5\r")
    terminal.wait_text("48 of 48", 5)
    case.capture(terminal, "full walkthrough reaches final question")
    key(terminal, b"5\r", "Review and save")
    key(terminal, b"\x1b", "How would you like CA")
    key(terminal, b"3\r")
    case.capture(terminal, "all 12 behavior sections are available")
    sections = ("Voice and personality", "Detail and explanation", "Initiative and completion",
        "Understanding and clarification", "Reasoning and judgment", "Tools and delegation",
        "Evidence and verification", "Failure and recovery", "Memory and learning",
        "Efficiency and budgets", "Data, permissions, and trust", "Repository and long-running work")
    assert all(section in terminal.screen.text() for section in sections), "section menu omitted a behavior area"
    key(terminal, b"\x1b[B" * 11 + b"\r", "Code changes")
    key(terminal, b"3\r")
    key(terminal, b"\x1b", "How would you like CA")
    terminal.resize(60, 20)
    overview(terminal)
    key(terminal, b"5\r", "Ctrl-S Use answer")
    additional = EXTRA + "\nUse clear examples for café, 東京, and e\u0301."
    key(terminal, b"\x1b[200~" + additional.encode() + b"\x1b[201~")
    case.capture(terminal, "additional instructions with multiline Unicode text")
    assert "PERSONALITY_INTERACTIVE_ADDITIONAL" in terminal.screen.text()
    assert "café, 東京, and e\u0301." in terminal.screen.text(), "Unicode text was not visibly preserved in the actual 60x20 editor"
    key(terminal, b"\x13", "How would you like CA")
    key(terminal, b"4\r", "Review and save")
    key(terminal, b"\x1b[6~")
    case.capture(terminal, "scrollable exact instruction review")
    assert "café, 東京, and e\u0301." in terminal.screen.text(), "Unicode text was not visibly preserved in the actual 60x20 review"
    save(terminal)
    text, active, value = metadata(personality_path(case.root))
    assert value["answers"]["voice.tone"] == {"Choice": "direct"}
    assert value["answers"]["repository.changes"] == {"Choice": "behavior"}
    assert value["additional"] == additional, value
    assert additional in active and "Judge implementation choices by the requested observable behavior" in active
    assert len(text.encode()) < 32 * 1024
    exit_key(terminal)
    case.finish(terminal)
    return {"actual_full_walkthrough_traversed_48_questions": True, "all_12_areas_visibly_available": True,
        "section_specific_answer_saved": True, "multiline_unicode_additional_instructions_saved_exactly": True,
        "unicode_visible_in_60x20_editor_and_review": True, "review_scrolling_and_explicit_save": True}


def named_isolation(case):
    default_path = personality_path(case.root)
    default_path.parent.mkdir(parents=True, mode=0o700)
    default_path.write_text("# Default only\n" + IMPORTED + "\n")
    default_path.chmod(0o600)
    original = default_path.read_bytes()
    terminal = case.terminal(profile="questionnaire-fixture")
    quick(terminal)
    custom(terminal, CUSTOM)
    review(terminal)
    save(terminal)
    case.capture(terminal, "named profile save confirmation")
    exit_key(terminal)
    case.finish(terminal)
    _, active, value = metadata(personality_path(case.root, "questionnaire-fixture"))
    assert value["answers"]["voice.tone"] == {"Custom": CUSTOM}
    assert CUSTOM in active and IMPORTED not in active
    assert default_path.read_bytes() == original, "named profile modified default steering"
    bare = case.terminal(profile="bare")
    bare.wait_exit()
    record = next(record for owned, record in case.terminals if owned is bare)
    record["expected_exit_code"] = 1
    case.frames.append({"label": "bare rejection", "width": 80, "height": 24, "text": bare.screen.text()})
    assert "bare" in bare.screen.text().lower(), "bare setup did not explain the profile restriction"
    case.finish(bare)
    assert not (case.root / "runtime/.config/ca/profiles/bare").exists()
    return {"named_profile_saves_in_its_own_directory": True, "default_personality_untouched": True,
        "bare_profile_rejects_setup_without_persistence": True}


def piped_cli(case):
    outputs = [run_cli(case.binary, case.root), run_cli(case.binary, case.root, "--json"),
        run_cli(case.binary, case.root, "setup"), run_cli(case.binary, case.root, "--json", "setup")]
    assert outputs[0]["exit_code"] == 0 and "CAQ-" in outputs[0]["stdout"], outputs[0]
    assert outputs[1]["exit_code"] == 0 and len(json.loads(outputs[1]["stdout"])) == 48, outputs[1]
    for output in outputs[2:]:
        assert output["exit_code"] != 0, output
        assert "interactive" in output["stderr"].lower() or "terminal" in output["stderr"].lower(), output
    assert not personality_path(case.root).exists(), "noninteractive inspection/rejection mutated steering"
    return {"piped_default_preserves_read_only_catalog": True, "piped_json_contains_all_48_questions": True,
        "explicit_setup_requires_terminal": True, "setup_rejects_json": True, "commands": outputs}


def conflict_preserves_external_edit(case):
    path = personality_path(case.root)
    path.parent.mkdir(parents=True, mode=0o700)
    path.write_text("# Existing personality\n" + IMPORTED + "\n")
    path.chmod(0o600)
    terminal = case.terminal()
    quick(terminal)
    custom(terminal, CUSTOM)
    review(terminal)
    changed = "# Changed outside the questionnaire\nPERSONALITY_EXTERNAL_CAS_MARKER\n"
    path.write_text(changed)
    key(terminal, b"\r", "changed")
    case.capture(terminal, "concurrent edit save refusal")
    assert path.read_text() == changed, "saving overwrote a concurrent personality edit"
    exit_key(terminal)
    case.finish(terminal)
    assert path.read_text() == changed, "cancelling after conflict overwrote the external edit"
    return {"external_edit_detected_before_write": True, "concurrent_editor_bytes_preserved": True}


def native_provider_steering(case):
    terminal, provider = case.native()
    terminal.command("/personality", "Quick setup", 5)
    case.capture(terminal, "native /personality opens editor")
    quick(terminal)
    custom(terminal, CUSTOM)
    review(terminal)
    save(terminal)
    case.capture(terminal, "native explicit save confirmation")
    _, active, value = metadata(personality_path(case.root))
    assert value["answers"]["voice.tone"] == {"Custom": CUSTOM} and CUSTOM in active
    assert not provider.snapshot(), "questionnaire interaction contacted the model"
    key(terminal, b"\x1b", "How would you like CA")
    key(terminal, b"6\r")
    terminal.command("/questionnaire", "Quick setup", 5)
    case.capture(terminal, "native /questionnaire alias")
    key(terminal, b"6\r")
    assert not provider.snapshot(), "alias or close interaction contacted the model"
    provider.arm(NEXT_PROMPT, text=NEXT_REPLY)
    terminal.command(NEXT_PROMPT, NEXT_REPLY, 15)
    terminal.settle()
    requests = provider.snapshot()
    assert len(requests) == 1 and requests[0]["user"] == NEXT_PROMPT, requests
    assert CUSTOM in "\n".join(requests[0]["system_messages"]), "saved answer missing from next real provider request"
    assert "ca-personality-preferences:v1" not in "\n".join(requests[0]["system_messages"]), "private wizard metadata reached the model"
    case.capture(terminal, "actual next model turn completes")
    return {"native_slash_command_and_alias_open_same_questionnaire": True,
        "wizard_interaction_sends_no_provider_requests": True, "saved_answer_reaches_next_actual_system_messages": True,
        "questionnaire_commands_are_not_provider_user_turns": True}


CASES = {"setup-back-edit-save-and-resume": setup_edit_and_resume,
    "quick-scope-and-partial-save": quick_scope_and_partial_save,
    "small-screen-discard-and-ctrl-c": small_screen_cancel,
    "full-walkthrough-sections-and-additional": full_walkthrough_sections_and_additional,
    "named-and-bare-isolation": named_isolation,
    "piped-and-json-behavior": piped_cli, "concurrent-edit-preservation": conflict_preserves_external_edit,
    "native-questionnaire-next-provider-request": native_provider_steering}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--receipt", type=Path)
    parser.add_argument("--baseline-only", action="store_true")
    parser.add_argument("--case", choices=CASES, action="append", help="Run a selected case; may be repeated.")
    args = parser.parse_args()
    if args.baseline_only and args.case:
        parser.error("--baseline-only and --case cannot be combined")
    binary = args.binary.resolve(strict=True)
    fixture_parent = "/private/tmp" if Path("/private/tmp").is_dir() else None
    root = Path(tempfile.mkdtemp(prefix="ca-personality-questionnaire-pty-", dir=fixture_parent)).resolve()
    root.chmod(0o700)
    receipt = {"schema": "ca.personality-questionnaire-pty-receipt.v1", "status": "running",
        "root": str(root), "binary": str(binary), "binary_sha256": binary_digest(binary), "cases": {},
        "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    if args.baseline_only:
        case_root = root / "baseline-interactive-setup"
        case_root.mkdir(mode=0o700)
        receipt["cases"]["baseline-interactive-setup"] = baseline_case(binary, case_root)
    else:
        cases = {name: CASES[name] for name in args.case} if args.case else CASES
        for name, run in cases.items():
            case_root = root / name
            case_root.mkdir(mode=0o700)
            case = Case(binary, case_root)
            result = {"status": "running"}
            try:
                result["checks"] = run(case)
                result["status"] = "passed"
            except Exception as error:
                result["status"] = "failed"
                result["error"] = str(error)
                result["screen"] = case.terminals[-1][0].screen.text() if case.terminals else "startup failed"
            finally:
                for terminal, record in case.terminals:
                    try:
                        case.finish(terminal)
                    except Exception as error:
                        result["status"] = "failed"
                        result.setdefault("error", "terminal cleanup failed: " + str(error))
                    if record.get("exit_code") != record["expected_exit_code"] or not record.get("terminal_restored"):
                        result["status"] = "failed"
                        result.setdefault("error", "terminal exit or termios restoration failed")
                for provider in case.providers:
                    provider.close()
                    result["provider_requests"] = provider.snapshot()
                    result["provider_failures"] = provider.failures
                    if provider.failures:
                        result["status"] = "failed"
                result["terminals"] = [record for _, record in case.terminals]
                result["frames"] = case.frames
                receipt["cases"][name] = result
    receipt["binary_sha256_after"] = binary_digest(binary)
    receipt["binary_identity_stable"] = receipt["binary_sha256_after"] == receipt["binary_sha256"]
    receipt["status"] = "passed" if receipt["binary_identity_stable"] and all(case["status"] == "passed" for case in receipt["cases"].values()) else "failed"
    receipt["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    output = root / "result.json"
    output.write_text(json.dumps(receipt, indent=2) + "\n")
    if args.receipt:
        target = args.receipt.resolve()
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(receipt, indent=2) + "\n")
    def compact_result(case):
        value = {key: item for key, item in case.items()
            if key not in ("screen", "frames", "provider_requests", "provider_failures", "checks")}
        if "checks" in case:
            value["checks"] = {key: item for key, item in case["checks"].items() if key != "commands"}
        if "error" in value:
            value["error"] = value["error"].splitlines()[0][:180]
        return value

    print(json.dumps({"status": receipt["status"], "root": str(root), "result": str(output),
        "binary_sha256": receipt["binary_sha256"],
        "cases": {name: compact_result(case) for name, case in receipt["cases"].items()}}, ensure_ascii=False), flush=True)
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
