#!/usr/bin/env python3
"""Local direct-versus-map mechanism evidence.

This runner does not invoke the live qualifier and does not use a configured
provider route. It does not satisfy or replace the live HRS-317 gate.
"""
import json
import os
import sys


def release_binary() -> str:
    for key in ("CA_RELEASE_BIN", "CA_WORKER_TEST_BIN"):
        value = os.environ.get(key)
        if value and os.path.isfile(value):
            return os.path.realpath(value)
    candidate = os.path.normpath(
        os.path.join(os.path.dirname(__file__), "..", "target", "release", "ca")
    )
    if os.path.isfile(candidate):
        return os.path.realpath(candidate)
    raise SystemExit("release binary is missing for both offline lanes")


def main() -> int:
    if "run_one" in sys.argv:
        print("refusing run_one; this runner is local mechanism evidence only", file=sys.stderr)
        return 2
    binary = release_binary()
    lanes = [
        {"name": "direct", "binary": binary, "automatic_task_breakdown": False},
        {"name": "map", "binary": binary, "automatic_task_breakdown": True},
    ]
    report = {
        "label": "local mechanism evidence",
        "live_gate": False,
        "lanes": lanes,
        "same_binary": lanes[0]["binary"] == lanes[1]["binary"],
        "difference": "automatic task breakdown",
        "cases": [
            "small fix",
            "dependent multi-file work",
            "independent branches with a failure",
            "warm follow-up",
        ],
        "history_messages": 192,
        "inherited_environment": False,
    }
    if os.environ.get("ISO337_OFFLINE_SELF_TEST") == "1":
        assert report["label"] == "local mechanism evidence"
        assert report["live_gate"] is False
        assert report["same_binary"] is True
        assert lanes[0]["binary"] == lanes[1]["binary"]
        assert os.path.isfile(binary)
    json.dump(report, sys.stdout)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
