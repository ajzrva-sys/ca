#!/usr/bin/env python3
"""Read-only evidence verification and recoverable source review artifacts."""
import argparse
import datetime
import difflib
import hashlib
import json
import os
from pathlib import Path
import tarfile


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()



def review_group(relative):
    """Review partitions have shared wiring, not independent installability."""
    if relative == "src/context.rs":
        return "M1-checkpoints"
    if relative == "src/session_access.rs":
        return "M2-ownership"
    if relative in {"src/recovery.rs", "src/undo.rs", "src/plan_feedback.rs", "src/branches.rs", "src/branch_handoff.rs", "src/session_navigation.rs"}:
        return "M3-recovery-feedback-navigation"
    if relative.startswith(("src/dashboard_", "src/tui/dashboard", "src/tui/snapshots/ca__tui__render_tests__agent_dashboard")):
        return "M4-dashboard-lifecycle"
    if relative in {"src/tool_discovery.rs", "src/tool_selection.rs"}:
        return "M5-discovery-reads"
    if relative.startswith(("src/memory", "src/local_models", "tests/fixtures/memory_recall/")):
        return "M6-recall"
    if relative.startswith(("src/automation", "src/schedule", "src/harness_budget")):
        return "M7-automations"
    if relative.startswith(("docs/", "tools/")) or relative in {"CONTINUITY.md", "NEXT_STEPS.md", "ARCHITECTURE.md", "TESTING.md"}:
        return "M8-verification-documentation"
    return "shared-integration"

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--verify-oracles-only", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    baseline = args.baseline.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(output, 0o700)
    oracle = json.loads((baseline / "oracle-manifest.json").read_text())["files"]
    mismatches = []
    for item in oracle:
        path = root / item["path"]
        if not path.is_file() or digest(path) != item["sha256"]:
            mismatches.append(item["path"])
    original_oracle_paths = {item["path"] for item in oracle}
    current_oracle_paths = set()
    for evidence_root in (root / "oracles", root / "reference"):
        for directory, children, names in os.walk(evidence_root, followlinks=False):
            for name in names:
                path = Path(directory) / name
                if path.is_file() and not path.is_symlink():
                    current_oracle_paths.add(str(path.relative_to(root)))
    additions = sorted(current_oracle_paths - original_oracle_paths)
    evidence = {"checked_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                "checked_files": len(oracle), "mismatches": mismatches,
                "added_files": additions, "unchanged": not mismatches and not additions}
    (output / "oracle-verification.json").write_text(json.dumps(evidence, indent=2) + "\n")
    if mismatches or additions:
        raise SystemExit("Oracle evidence differs; review oracle-verification.json before release.")
    if args.verify_oracles_only:
        os.chmod(output / "oracle-verification.json", 0o600)
        print(json.dumps(evidence, sort_keys=True))
        return

    excluded = {"target", "oracles", "reference", "scratch", "work", ".git", ".codex", ".agents", ".aws"}
    files = []
    for directory, children, names in os.walk(root, followlinks=False):
        children[:] = sorted(name for name in children if name not in excluded)
        for name in sorted(names):
            path = Path(directory) / name
            if path.is_file() and not path.is_symlink() and name != ".DS_Store":
                files.append(path)
    manifest = [{"path": str(path.relative_to(root)), "sha256": digest(path), "bytes": path.stat().st_size}
                for path in files]
    before = {item["path"]: item for item in json.loads((baseline / "source-manifest.json").read_text())["files"]}
    after = {item["path"]: item for item in manifest}
    changed = sorted(path for path in before.keys() | after.keys()
                     if Path(path).name != ".DS_Store" and before.get(path, {}).get("sha256") != after.get(path, {}).get("sha256"))
    (output / "source-manifest.json").write_text(json.dumps({"files": manifest}, indent=2) + "\n")
    (output / "changed-files.json").write_text(json.dumps(changed, indent=2) + "\n")
    with tarfile.open(baseline / "source-baseline.tar.gz", "r:gz") as original:
        members = {member.name.removeprefix("./"): member for member in original.getmembers() if member.isfile()}
        patches = []
        groups = {}
        for relative in changed:
            old = original.extractfile(members[relative]).read() if relative in members else b""
            new = (root / relative).read_bytes() if relative in after else b""
            try:
                old_text, new_text = old.decode("utf-8"), new.decode("utf-8")
            except UnicodeDecodeError:
                patch = f"Binary content changed: {relative}\n"
            else:
                patch = "".join(difflib.unified_diff(old_text.splitlines(keepends=True), new_text.splitlines(keepends=True),
                                                 fromfile=f"baseline/{relative}", tofile=f"candidate/{relative}"))
            patches.append(patch)
            group = groups.setdefault(review_group(relative), {"paths": [], "patches": []})
            group["paths"].append(relative)
            group["patches"].append(patch)
        (output / "source-changes.patch").write_text("".join(patches))
        review = output / "milestones"
        review.mkdir(mode=0o700, exist_ok=True)
        index = {"scope": "Review partitions; shared wiring spans milestones. Apply the complete candidate, not an isolated partition.", "groups": {}}
        for name, group in sorted(groups.items()):
            path = review / f"{name}.patch"
            path.write_text("".join(group["patches"]))
            os.chmod(path, 0o600)
            index["groups"][name] = {"paths": group["paths"], "patch_sha256": digest(path)}
        (review / "index.json").write_text(json.dumps(index, indent=2) + "\n")
        os.chmod(review / "index.json", 0o600)
    with tarfile.open(output / "source-candidate.tar.gz", "w:gz") as archive:
        for path in files:
            archive.add(path, arcname=str(path.relative_to(root)), recursive=False)
    for path in files:
        relative = str(path.relative_to(root))
        if digest(path) != after[relative]["sha256"]:
            raise SystemExit("Source changed during capture; rerun after writers finish.")
    for name in ["source-candidate.tar.gz", "source-changes.patch", "source-manifest.json", "changed-files.json", "oracle-verification.json"]:
        os.chmod(output / name, 0o600)
    print(json.dumps({"oracle_files_unchanged": len(oracle), "source_files": len(files), "changed_files": len(changed),
                      "artifact_directory": str(output)}, sort_keys=True))


if __name__ == "__main__":
    main()
