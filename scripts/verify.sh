#!/bin/sh
# Portable entry point; all evidence and application state belong to the run.
set -eu
verify_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ "${1:-}" = "--self-test" ]; then
    shift
    exec python3 "$verify_root/tools/verify_selftest.py" "$@"
fi
exec python3 "$verify_root/scripts/verify.py" "$@"
