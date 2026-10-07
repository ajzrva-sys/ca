#!/usr/bin/env bash
# Bounded Hermes-style supervisor loop for codex-team.
# Requires clean --json stdout (protocol_version: 1 cycle receipts).
set -euo pipefail

REPO="${1:?usage: hermes_supervisor_loop.sh <repo> [max_cycles]}"
BIN="${CODEX_TEAM:-codex-team}"
MAX="${2:-5}"

for i in $(seq 1 "$MAX"); do
  echo "=== cycle $i/$MAX ===" >&2
  set +e
  receipt="$("$BIN" step --repo "$REPO" --json </dev/null)"
  step_rc=$?
  set -e
  printf '%s\n' "$receipt"
  outcome="$(printf '%s\n' "$receipt" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d.get("outcome",""))' 2>/dev/null || true)"
  case "$outcome" in
    continue) ;;
    paused)
      echo "paused: notify human; run: $BIN disposition --repo $REPO --decision <preserve|recheck|gate|keep-paused> --json" >&2
      exit 2
      ;;
    no_work|stopped)
      echo "done: $outcome" >&2
      exit 0
      ;;
    error)
      err_kind="$(printf '%s\n' "$receipt" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d.get("error_kind") or "error")' 2>/dev/null || echo error)"
      err_summary="$(printf '%s\n' "$receipt" | python3 -c 'import sys,json; d=json.load(sys.stdin); print((d.get("error_summary") or "")[:200])' 2>/dev/null || true)"
      echo "error: kind=${err_kind} rc=${step_rc} ${err_summary}" >&2
      echo "stop: do not blind-retry; fix provider/schema or raise [routing].local_provider_failure_budget" >&2
      exit 1
      ;;
    *)
      echo "unknown or missing outcome: '${outcome}' (step_rc=${step_rc})" >&2
      exit 1
      ;;
  esac
done

echo "cycle cap reached ($MAX)" >&2
exit 0
