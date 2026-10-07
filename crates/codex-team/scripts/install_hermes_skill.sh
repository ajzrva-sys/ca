#!/usr/bin/env bash
# Install in-repo Hermes skills into ~/.hermes/skills/
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HOME_DIR="${HERMES_HOME:-$HOME/.hermes}"

install_skill() {
  local name="$1"
  local src="${ROOT}/skills/${name}"
  local dest="${HOME_DIR}/skills/${name}"
  if [[ ! -f "${src}/SKILL.md" ]]; then
    echo "missing ${src}/SKILL.md" >&2
    return 1
  fi
  mkdir -p "${dest}"
  cp -f "${src}/SKILL.md" "${dest}/SKILL.md"
  echo "Installed Hermes skill '${name}' -> ${dest}"
}

install_skill codex-team
install_skill codex-team-boss || true

# Schema references beside primary skill
if [[ -d "${ROOT}/schemas" ]]; then
  mkdir -p "${HOME_DIR}/skills/codex-team/references"
  for f in cycle_receipt status_receipt disposition_receipt doctor_receipt; do
    if [[ -f "${ROOT}/schemas/${f}.schema.json" ]]; then
      cp -f "${ROOT}/schemas/${f}.schema.json" \
        "${HOME_DIR}/skills/codex-team/references/${f}.schema.json"
    fi
  done
fi

echo "Verify: hermes skills list | grep -i codex-team"
echo "Use:   hermes chat -s codex-team,codex-team-boss -q 'status of supervised repo'"
