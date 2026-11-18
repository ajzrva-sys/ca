#!/usr/bin/env python3
"""Pre-Cargo ceilings for comments, TUI blocking calls, and oracle includes."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

TREES = ("ca-hrs-252/", "oracles/", "reference/")
INCLUDE_MACROS = {"include", "include_str", "include_bytes"}
BLOCKING = {"block_on", "block_in_place"}
COMMENT_INCREASE = (
    "pyramid: {path} has {n} code comments; the ceiling is {b}. "
    "Comments are not a constraint. Delete the comment. If this stops a "
    "repeated mistake, add a check in scripts/check-pyramid.py whose error "
    "names the cause and the fix."
)
COMMENT_DECREASE = (
    "pyramid: {path} fell from {b} to {n}. "
    "Run python3 scripts/check-pyramid.py --update and commit the lower ceiling."
)
UPDATE_REFUSAL = (
    "pyramid: --update cannot raise {path} from {b} to {n}. "
    "Ceilings only move down. Delete the new comment or blocking call."
)
TUI_BLOCK = (
    "pyramid: {path}:{line} calls {name}. src/tui must stay on the event loop. "
    "Move the wait onto the existing tokio task and send the result as an event. "
    "Do not introduce a new blocking call even in a test under src/tui/."
)
MAIN_BLOCK = (
    "pyramid: src/main.rs has {n} block_on/block_in_place calls; the ceiling is {b}. "
    "That ceiling is the one existing test call and is temporary. Do not copy it. "
    "The turn loop must not wait synchronously. Move the work onto the tokio task."
)
ORACLE_INCLUDE = (
    "pyramid: {path}:{line} compiles {tree} into ca. Those trees are read-only "
    "evidence. Reproduce the behavior with a fixture under tests/ or src/."
)


def _ident_char(char: str) -> bool:
    return char.isalnum() or char == "_"


def _is_doc_block(text: str, index: int) -> bool:
    if not text.startswith("/*", index) or index + 2 >= len(text):
        return False
    third = text[index + 2]
    fourth = text[index + 3] if index + 3 < len(text) else ""
    if third == "*" and fourth not in {"*", "/"}:
        return True
    return third == "!" and fourth != "*"


def _skip_block(text: str, index: int) -> int:
    depth = 1
    cursor = index + 2
    limit = len(text)
    while cursor < limit and depth:
        if text.startswith("/*", cursor):
            depth += 1
            cursor += 2
        elif text.startswith("*/", cursor):
            depth -= 1
            cursor += 2
        else:
            cursor += 1
    return cursor


def _skip_gap(text: str, index: int) -> int:
    limit = len(text)
    while index < limit:
        if text[index] in " \t\r\n":
            index += 1
            continue
        if text.startswith("//", index):
            newline = text.find("\n", index)
            index = limit if newline < 0 else newline + 1
            continue
        if text.startswith("/*", index):
            index = _skip_block(text, index)
            continue
        break
    return index


def _read_cooked(text: str, index: int) -> tuple[int, str]:
    cursor = index + 1
    body: list[str] = []
    limit = len(text)
    while cursor < limit:
        char = text[cursor]
        if char == "\\":
            if cursor + 1 >= limit:
                return limit, "".join(body)
            body.append(text[cursor + 1])
            cursor += 2
            continue
        if char == '"':
            return cursor + 1, "".join(body)
        if char == "\n":
            return cursor, "".join(body)
        body.append(char)
        cursor += 1
    return cursor, "".join(body)


def _read_string(text: str, index: int) -> tuple[int, str] | None:
    if index > 0 and _ident_char(text[index - 1]):
        return None
    cursor = index
    limit = len(text)
    if cursor < limit and text[cursor] in "bc" and cursor + 1 < limit and text[cursor + 1] in '"r':
        cursor += 1
    if cursor < limit and text[cursor] == "r":
        hashes = 0
        probe = cursor + 1
        while probe < limit and text[probe] == "#":
            hashes += 1
            probe += 1
        if probe < limit and text[probe] == '"':
            probe += 1
            closer = '"' + ("#" * hashes)
            end = text.find(closer, probe)
            if end < 0:
                return limit, text[probe:]
            return end + len(closer), text[probe:end]
        return None
    if cursor < limit and text[cursor] == '"':
        return _read_cooked(text, cursor)
    return None


def _read_char_or_lifetime(text: str, index: int) -> int | None:
    if index > 0 and _ident_char(text[index - 1]):
        return None
    cursor = index
    limit = len(text)
    if cursor < limit and text[cursor] == "b" and cursor + 1 < limit and text[cursor + 1] == "'":
        cursor += 1
    if cursor >= limit or text[cursor] != "'":
        return None
    probe = cursor + 1
    if probe >= limit:
        return None
    if text[probe] == "\\":
        probe += 1
        if probe >= limit:
            return None
        if text[probe] == "u":
            brace = text.find("}", probe)
            if brace < 0 or brace + 1 >= limit or text[brace + 1] != "'":
                return None
            return brace + 2
        probe += 1
        if probe < limit and text[probe] == "'":
            return probe + 1
        return None
    if probe + 1 < limit and text[probe + 1] == "'":
        return probe + 2
    if text[probe].isalpha() or text[probe] == "_":
        probe += 1
        while probe < limit and _ident_char(text[probe]):
            probe += 1
        return probe
    return None


def _read_ident(text: str, index: int) -> tuple[str, int] | None:
    if index >= len(text) or not (text[index].isalpha() or text[index] == "_"):
        return None
    cursor = index + 1
    while cursor < len(text) and _ident_char(text[cursor]):
        cursor += 1
    return text[index:cursor], cursor


def _tree_in(body: str) -> str | None:
    for tree in TREES:
        if tree in body:
            return tree
    return None


class Scan:
    def __init__(self, text: str) -> None:
        self.text = text
        self.comments = 0
        self.blocks: list[tuple[int, str]] = []
        self.includes: list[tuple[int, str]] = []
        self._scan()

    def _scan(self) -> None:
        text = self.text
        limit = len(text)
        index = 0
        line = 1
        while index < limit:
            if text.startswith("//", index):
                if not (index + 2 < limit and text[index + 2] in "/!"):
                    self.comments += 1
                newline = text.find("\n", index)
                nxt = limit if newline < 0 else newline + 1
                line += text.count("\n", index, nxt)
                index = nxt
                continue
            if text.startswith("/*", index):
                if not _is_doc_block(text, index):
                    self.comments += 1
                nxt = _skip_block(text, index)
                line += text.count("\n", index, nxt)
                index = nxt
                continue
            string = _read_string(text, index)
            if string is not None:
                nxt = string[0]
                line += text.count("\n", index, nxt)
                index = nxt if nxt > index else index + 1
                continue
            lifetime = _read_char_or_lifetime(text, index)
            if lifetime is not None:
                line += text.count("\n", index, lifetime)
                index = lifetime if lifetime > index else index + 1
                continue
            if text[index] == "#":
                self._path_attribute(index, line)
                index += 1
                continue
            ident = _read_ident(text, index)
            if ident is not None:
                name, nxt = ident
                if name in BLOCKING:
                    self.blocks.append((line, name))
                if name in INCLUDE_MACROS:
                    self._include(nxt, line)
                line += text.count("\n", index, nxt)
                index = nxt
                continue
            if text[index] == "\n":
                line += 1
            index += 1

    def _include(self, index: int, line: int) -> None:
        text = self.text
        cursor = _skip_gap(text, index)
        if cursor >= len(text) or text[cursor] != "!":
            return
        cursor = _skip_gap(text, cursor + 1)
        if cursor >= len(text) or text[cursor] != "(":
            return
        cursor = _skip_gap(text, cursor + 1)
        parsed = _read_string(text, cursor)
        if parsed is None:
            return
        tree = _tree_in(parsed[1])
        if tree is not None:
            self.includes.append((line, tree))

    def _path_attribute(self, index: int, line: int) -> None:
        text = self.text
        cursor = _skip_gap(text, index + 1)
        if cursor >= len(text) or text[cursor] != "[":
            return
        cursor = _skip_gap(text, cursor + 1)
        ident = _read_ident(text, cursor)
        if ident is None or ident[0] != "path":
            return
        cursor = _skip_gap(text, ident[1])
        if cursor >= len(text) or text[cursor] != "=":
            return
        cursor = _skip_gap(text, cursor + 1)
        parsed = _read_string(text, cursor)
        if parsed is None:
            return
        tree = _tree_in(parsed[1])
        if tree is not None:
            self.includes.append((line, tree))


def count_comments(text: str) -> int:
    return Scan(text).comments


def blocking_calls(text: str) -> list[tuple[int, str]]:
    return Scan(text).blocks


def forbidden_includes(text: str) -> list[tuple[int, str]]:
    return Scan(text).includes


def rust_files(root: Path) -> list[Path]:
    found: list[Path] = []
    src = root / "src"
    if src.is_dir():
        found.extend(path for path in src.rglob("*.rs") if path.is_file() and not path.is_symlink())
    crates = root / "crates"
    if crates.is_dir():
        for crate in crates.iterdir():
            crate_src = crate / "src"
            if crate_src.is_dir():
                found.extend(
                    path for path in crate_src.rglob("*.rs") if path.is_file() and not path.is_symlink()
                )
    return sorted(found)


def relative(root: Path, path: Path) -> str:
    return path.relative_to(root).as_posix()


def scan_tree(root: Path) -> tuple[dict[str, int], int, list[tuple[str, int, str]], list[tuple[str, int, str]]]:
    comments: dict[str, int] = {}
    main_blocks = 0
    tui: list[tuple[str, int, str]] = []
    includes: list[tuple[str, int, str]] = []
    for path in rust_files(root):
        rel = relative(root, path)
        scan = Scan(path.read_text(encoding="utf-8"))
        if scan.comments:
            comments[rel] = scan.comments
        if rel == "src/main.rs":
            main_blocks = len(scan.blocks)
        if rel.startswith("src/tui/"):
            tui.extend((rel, line, name) for line, name in scan.blocks)
        includes.extend((rel, line, tree) for line, tree in scan.includes)
    tui.sort()
    includes.sort()
    return comments, main_blocks, tui, includes


def baseline_path(root: Path) -> Path:
    return root / "scripts" / "pyramid-baseline.txt"


def load_baseline(path: Path) -> tuple[dict[str, int], int, bool, list[str]]:
    if not path.is_file():
        return {}, 0, False, []
    comments: dict[str, int] = {}
    block: int | None = None
    errors: list[str] = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line:
            continue
        parts = line.split(" ")
        bad = (
            f"pyramid: scripts/pyramid-baseline.txt:{number} is not a ceiling record. "
            "Regenerate with python3 scripts/check-pyramid.py --update after the tree is clean."
        )
        if len(parts) != 3 or not parts[2].isdigit():
            errors.append(bad)
            continue
        kind, name, count_text = parts
        count = int(count_text)
        if kind == "comment" and name.startswith(("src/", "crates/")) and ".." not in name.split("/"):
            if name in comments:
                errors.append(bad)
            else:
                comments[name] = count
        elif kind == "block" and name == "main.rs" and block is None:
            block = count
        else:
            errors.append(bad)
    return comments, 0 if block is None else block, True, errors


def render_baseline(comments: dict[str, int], block: int) -> str:
    lines = [f"block main.rs {block}"]
    lines.extend(f"comment {path} {comments[path]}" for path in sorted(comments) if comments[path])
    lines.sort()
    return "\n".join(lines) + "\n"


def comment_messages(current: dict[str, int], previous: dict[str, int]) -> list[str]:
    messages: list[str] = []
    for path in sorted(set(current) | set(previous)):
        now = current.get(path, 0)
        before = previous.get(path, 0)
        if now > before:
            messages.append(COMMENT_INCREASE.format(path=path, n=now, b=before))
        elif now < before:
            messages.append(COMMENT_DECREASE.format(path=path, n=now, b=before))
    return messages


def block_message(now: int, before: int) -> str | None:
    if now > before:
        return MAIN_BLOCK.format(n=now, b=before)
    if now < before:
        return COMMENT_DECREASE.format(path="src/main.rs", n=now, b=before)
    return None


def findings(root: Path) -> tuple[list[str], dict[str, int], int, bool]:
    comments, main_blocks, tui, includes = scan_tree(root)
    previous, block_ceiling, present, broken = load_baseline(baseline_path(root))
    messages = list(broken)
    if not broken:
        messages.extend(comment_messages(comments, previous))
        messages.extend(TUI_BLOCK.format(path=path, line=line, name=name) for path, line, name in tui)
        mismatch = block_message(main_blocks, block_ceiling)
        if mismatch is not None:
            messages.append(mismatch)
        messages.extend(ORACLE_INCLUDE.format(path=path, line=line, tree=tree) for path, line, tree in includes)
    return messages, comments, main_blocks, present


def update_refusals(current: dict[str, int], previous: dict[str, int], main_now: int, main_before: int) -> list[str]:
    messages: list[str] = []
    for path in sorted(set(current) | set(previous)):
        now = current.get(path, 0)
        before = previous.get(path, 0)
        if now > before:
            messages.append(UPDATE_REFUSAL.format(path=path, n=now, b=before))
    if main_now > main_before:
        messages.append(UPDATE_REFUSAL.format(path="src/main.rs", n=main_now, b=main_before))
    return messages


def write_baseline(root: Path, comments: dict[str, int], block: int) -> None:
    path = baseline_path(root)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(render_baseline(comments, block), encoding="utf-8")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--update", action="store_true")
    parser.add_argument("--root", type=Path)
    args = parser.parse_args(argv)
    root = (args.root or Path(__file__).resolve().parents[1]).resolve()
    if not root.is_dir():
        print(f"pyramid: root is not a directory: {root}", file=sys.stderr)
        return 2
    comments, main_blocks, tui, includes = scan_tree(root)
    path = baseline_path(root)
    previous, block_ceiling, present, broken = load_baseline(path)
    if args.update:
        if broken:
            print("\n".join(broken))
            return 1
        structural = [TUI_BLOCK.format(path=item[0], line=item[1], name=item[2]) for item in tui]
        structural.extend(ORACLE_INCLUDE.format(path=item[0], line=item[1], tree=item[2]) for item in includes)
        refusals = update_refusals(comments, previous, main_blocks, block_ceiling) if present else []
        if structural or refusals:
            print("\n".join([*structural, *refusals]))
            return 1
        write_baseline(root, comments, main_blocks)
        return 0
    messages, _, _, _ = findings(root)
    if messages:
        print("\n".join(messages))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
