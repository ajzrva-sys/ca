#!/usr/bin/env python3
"""Fixtures for scripts/check-pyramid.py. Discovered by verify workflow-tests."""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "check-pyramid.py"


def load_checker():
    spec = importlib.util.spec_from_file_location("check_pyramid", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


CHECKER = load_checker()


def run(root: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(SCRIPT), "--root", str(root), *args],
        capture_output=True,
        text=True,
        check=False,
    )


def write_tree(root: Path, files: dict[str, str], baseline: str | None = None) -> None:
    for name, content in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
    if baseline is not None:
        path = root / "scripts" / "pyramid-baseline.txt"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(baseline, encoding="utf-8")


class ScannerTests(unittest.TestCase):
    def test_strings_and_doc_comments_are_not_code_comments(self) -> None:
        text = "\n".join(
            [
                r'const A: &str = "https://example.com";',
                r'const B: &str = r#"https://example.com"#;',
                r'const C: &str = r##"// not a comment"##;',
                r'const D: &str = r"// not";',
                r'const E: &str = "a\"//b";',
                "/// note // still a doc",
                "//! inner // still a doc",
                "/** doc // still */",
                "/*! inner doc */",
                "const F: &str = r#\"",
                "// not",
                "/* no */",
                "\"#;",
            ]
        )
        self.assertEqual(CHECKER.count_comments(text), 0)
        self.assertEqual(CHECKER.blocking_calls(text), [])

    def test_nested_block_comment_counts_once(self) -> None:
        self.assertEqual(CHECKER.count_comments("/* outer /* inner */ still */"), 1)
        self.assertEqual(CHECKER.count_comments("/**/"), 1)
        self.assertEqual(CHECKER.count_comments("/*** still */"), 1)

    def test_one_line_comment_counts_as_one(self) -> None:
        self.assertEqual(CHECKER.count_comments("// one\nfn a() {}\n"), 1)
        self.assertEqual(CHECKER.count_comments("fn a() {} // one\n"), 1)

    def test_blocking_call_inside_string_or_comment_is_ignored(self) -> None:
        text = "\n".join(
            [
                "fn keep() {",
                '    let s = "block_on";',
                "    // block_in_place",
                '    let t = r##"block_on"##;',
                "    /* block_on */",
                "}",
            ]
        )
        self.assertEqual(CHECKER.blocking_calls(text), [])
        self.assertEqual(CHECKER.count_comments(text), 2)

    def test_char_literal_slash_is_not_a_comment(self) -> None:
        self.assertEqual(CHECKER.count_comments("fn a() { let c = '/'; }\n"), 0)
        self.assertEqual(CHECKER.blocking_calls("fn a() { runtime.block_on(work); }\n"), [(1, "block_on")])


class GateTests(unittest.TestCase):
    def test_comment_increase_names_the_ceiling_and_the_fix(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_tree(
                root,
                {"src/a.rs": "// one\nfn a() {}\n", "src/main.rs": "fn main() {}\n"},
                "block main.rs 0\n",
            )
            result = run(root)
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "pyramid: src/a.rs has 1 code comments; the ceiling is 0. "
            "Comments are not a constraint. Delete the comment. If this stops a "
            "repeated mistake, add a check in scripts/check-pyramid.py whose error "
            "names the cause and the fix.",
            result.stdout,
        )

    def test_second_comment_fails_an_existing_ceiling(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_tree(
                root,
                {"src/a.rs": "// one\n// two\n", "src/main.rs": "fn main() {}\n"},
                "block main.rs 0\ncomment src/a.rs 1\n",
            )
            result = run(root)
        self.assertEqual(result.returncode, 1)
        self.assertIn("pyramid: src/a.rs has 2 code comments; the ceiling is 1.", result.stdout)

    def test_decrease_tells_you_to_update(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_tree(
                root,
                {"src/a.rs": "fn a() {}\n", "src/main.rs": "fn main() {}\n"},
                "block main.rs 0\ncomment src/a.rs 2\n",
            )
            result = run(root)
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "pyramid: src/a.rs fell from 2 to 0. "
            "Run python3 scripts/check-pyramid.py --update and commit the lower ceiling.",
            result.stdout,
        )

    def test_update_refuses_to_raise_and_leaves_the_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            baseline = "block main.rs 0\ncomment src/a.rs 1\n"
            write_tree(root, {"src/a.rs": "// one\n// two\n", "src/main.rs": "fn main() {}\n"}, baseline)
            result = run(root, "--update")
            kept = (root / "scripts" / "pyramid-baseline.txt").read_text(encoding="utf-8")
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "pyramid: --update cannot raise src/a.rs from 1 to 2. "
            "Ceilings only move down. Delete the new comment or blocking call.",
            result.stdout,
        )
        self.assertEqual(kept, baseline)

    def test_update_lowers_and_is_stable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_tree(
                root,
                {"src/a.rs": "fn a() {}\n", "src/b.rs": "// kept\n", "src/main.rs": "fn main() {}\n"},
                "block main.rs 0\ncomment src/a.rs 3\ncomment src/b.rs 1\n",
            )
            first = run(root, "--update")
            text = (root / "scripts" / "pyramid-baseline.txt").read_bytes()
            second = run(root, "--update")
            again = (root / "scripts" / "pyramid-baseline.txt").read_bytes()
            check = run(root)
        self.assertEqual(first.returncode, 0)
        self.assertEqual(second.returncode, 0)
        self.assertEqual(check.returncode, 0)
        self.assertEqual(text, again)
        self.assertTrue(text.endswith(b"\n"))
        lines = text.decode().splitlines()
        self.assertEqual(lines, sorted(lines))
        self.assertEqual(lines, ["block main.rs 0", "comment src/b.rs 1"])
        self.assertTrue(all(not line.startswith("/") for line in lines))

    def test_tui_block_on_fails_even_in_a_test(self) -> None:
        source = "#[cfg(test)]\nfn t() { runtime.block_on(work); }\n"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_tree(root, {"src/tui/input.rs": source, "src/main.rs": "fn main() {}\n"}, "block main.rs 0\n")
            result = run(root)
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "pyramid: src/tui/input.rs:2 calls block_on. src/tui must stay on the event loop. "
            "Move the wait onto the existing tokio task and send the result as an event. "
            "Do not introduce a new blocking call even in a test under src/tui/.",
            result.stdout,
        )

    def test_tui_blocking_token_only_in_string_or_comment_does_not_fire(self) -> None:
        source = "\n".join(
            [
                "fn keep() {",
                '    let s = "block_on";',
                "    // block_in_place",
                '    let t = r##"block_on"##;',
                "}",
                "",
            ]
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_tree(
                root,
                {"src/tui/input.rs": source, "src/main.rs": "fn main() {}\n"},
                "block main.rs 0\ncomment src/tui/input.rs 1\n",
            )
            result = run(root)
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_main_ceiling_says_the_existing_call_is_temporary(self) -> None:
        source = "fn main() {}\nfn t() { runtime.block_on(x); runtime.block_in_place(y); }\n"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_tree(root, {"src/main.rs": source}, "block main.rs 1\n")
            result = run(root)
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "pyramid: src/main.rs has 2 block_on/block_in_place calls; the ceiling is 1. "
            "That ceiling is the one existing test call and is temporary. Do not copy it. "
            "The turn loop must not wait synchronously. Move the work onto the tokio task.",
            result.stdout,
        )

    def test_oracle_includes_fail_and_ordinary_strings_do_not(self) -> None:
        source = "\n".join(
            [
                'const A: &str = include_str!("../../oracles/x.rs");',
                'const B: &[u8] = include_bytes!("../../oracles/x.bin");',
                'const C: &str = include!("ca-hrs-252/lib.rs");',
                '#[path = "reference/foo.rs"]',
                "mod foreign;",
                'const S: &str = "oracles/secret";',
                'const T: &str = include_str!("prompts/system.md");',
                "",
            ]
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_tree(root, {"src/lib.rs": source, "src/main.rs": "fn main() {}\n"}, "block main.rs 0\n")
            result = run(root)
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "pyramid: src/lib.rs:1 compiles oracles/ into ca. Those trees are read-only "
            "evidence. Reproduce the behavior with a fixture under tests/ or src/.",
            result.stdout,
        )
        self.assertIn("pyramid: src/lib.rs:2 compiles oracles/ into ca.", result.stdout)
        self.assertIn("pyramid: src/lib.rs:3 compiles ca-hrs-252/ into ca.", result.stdout)
        self.assertIn("pyramid: src/lib.rs:4 compiles reference/ into ca.", result.stdout)
        self.assertNotIn("prompts/system.md", result.stdout)
        self.assertEqual(result.stdout.count("oracles/secret"), 0)

    def test_clean_tree_passes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_tree(root, {"src/a.rs": "fn a() {}\n", "src/main.rs": "fn main() {}\n"})
            created = run(root, "--update")
            checked = run(root)
        self.assertEqual(created.returncode, 0, created.stdout)
        self.assertEqual(checked.returncode, 0, checked.stdout)


if __name__ == "__main__":
    unittest.main()
