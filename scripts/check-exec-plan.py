#!/usr/bin/env python3
"""Check one ExecPlan for the GRD-010 section names. Does not import product crates."""
import re
import sys
from pathlib import Path

# Canonical name, then headings that also satisfy it (.agent/PLANS.md aliases).
REQUIRED = (
    ("Goal", ("Goal", "Purpose", "Big Picture")),
    (
        "Acceptance Criteria or Validation",
        (
            "Acceptance Criteria",
            "Validation",
            "Validation and Acceptance",
        ),
    ),
    (
        "Recovery and Rollback",
        ("Recovery and Rollback", "Idempotence and Recovery"),
    ),
)

HEADING = re.compile(r"^##\s+(.+?)\s*$")


def headings(text):
    found = []
    for line in text.splitlines():
        match = HEADING.match(line)
        if match:
            found.append(match.group(1))
    return found


def missing(names):
    present = set(names)
    for canonical, aliases in REQUIRED:
        if not any(alias in present for alias in aliases):
            return canonical
    return None


def main(argv):
    if len(argv) != 2:
        print("usage: python3 scripts/check-exec-plan.py <plan.md>", file=sys.stderr)
        return 2
    path = Path(argv[1])
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as error:
        print(f"active ExecPlan missing required section: Goal ({error})", file=sys.stderr)
        return 1
    absent = missing(headings(text))
    if absent:
        print(f"active ExecPlan missing required section: {absent}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
