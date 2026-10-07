#!/bin/bash
# Install a freshly validated, signed candidate recoverably. --check is read-only.
set -euo pipefail

repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
candidate=${CA_INSTALL_CANDIDATE:-"$repository/target/release/ca"}
destination=${CA_INSTALL_DESTINATION:-"$(command -v ca)"}
mode=${1:-install}
if [[ $# -gt 1 || ( "$mode" != install && "$mode" != --check ) ]]; then
    printf 'Usage: bash tools/bug257_install.sh [--check]\n' >&2
    exit 2
fi

sha() { /usr/bin/shasum -a 256 "$1" | /usr/bin/awk '{print $1}'; }
# Resolve current artifacts once, then refuse changes during staging.
[[ -f "$candidate" && ! -L "$candidate" && -x "$candidate" ]]
[[ -f "$destination" && ! -L "$destination" && -x "$destination" ]]
expected_sha=$(sha "$candidate")
expected_prior_sha=$(sha "$destination")
expected_version=$("$candidate" --version)
expected_mode=$(/usr/bin/stat -f %Lp "$candidate")
verify_candidate() {
    local artifact=$1
    [[ -f "$artifact" && ! -L "$artifact" && -x "$artifact" ]]
    [[ $(/usr/bin/stat -f %u "$artifact") == "$(/usr/bin/id -u)" ]]
    [[ $(sha "$artifact") == "$expected_sha" ]]
    [[ $(/usr/bin/stat -f %Lp "$artifact") == "$expected_mode" ]]
    /usr/bin/codesign --verify --strict --verbose=3 "$artifact"
    [[ $("$artifact" --version) == "$expected_version" ]]
    "$artifact" --help >/dev/null
}

# Refuse to overwrite a subsequently installed release or an unexpected path.
[[ $(command -v ca) == "$destination" ]]
[[ -f "$destination" && ! -L "$destination" && -x "$destination" ]]
[[ $(/usr/bin/stat -f %u "$destination") == "$(/usr/bin/id -u)" ]]
[[ $(sha "$destination") == "$expected_prior_sha" ]]
/usr/bin/codesign --verify --strict --verbose=3 "$destination"
verify_candidate "$candidate"
if [[ "$mode" == --check ]]; then
    printf 'Verified candidate %s; installed binary unchanged.\n' "$expected_sha"
    exit 0
fi

stamp=$(/bin/date -u +%Y%m%dT%H%M%SZ)
stage=$(/usr/bin/mktemp -d "$(dirname -- "$destination")/.ca-release-stage.XXXXXX")
backup="$stage/ca.recovery-ORC258-$stamp-$expected_prior_sha"
installed=0
rollback() {
    local status=$?
    trap - EXIT
    if [[ $status -ne 0 && $installed == 1 ]]; then
        printf 'Post-install validation failed; restoring %s\n' "$backup" >&2
        if /bin/cp -p "$backup" "$stage/ca-rollback" &&
           /bin/mv -f "$stage/ca-rollback" "$destination" &&
           /usr/bin/codesign --force --sign - "$destination" &&
           /usr/bin/codesign --verify --strict --verbose=3 "$destination" &&
           [[ $(sha "$destination") == "$expected_prior_sha" ]]; then
            printf 'Previous binary restored; recovery/staging retained at %s\n' "$stage" >&2
        else
            printf 'Rollback needs attention; preserved recovery: %s\n' "$backup" >&2
        fi
    elif [[ $status -ne 0 ]]; then
        printf 'Installation stopped before replacement; staging retained at %s\n' "$stage" >&2
    fi
    exit "$status"
}
trap rollback EXIT

/bin/cp -p "$destination" "$backup"
[[ $(sha "$backup") == "$expected_prior_sha" ]]
/bin/cp -p "$candidate" "$stage/ca"
[[ $(sha "$stage/ca") == "$expected_sha" ]]
/usr/bin/codesign --force --sign - "$stage/ca"
verify_candidate "$stage/ca"
[[ $(sha "$destination") == "$expected_prior_sha" ]]
/bin/mv -f "$stage/ca" "$destination"
installed=1
# Required post-copy signing at the actual installed destination.
/usr/bin/codesign --force --sign - "$destination"
verify_candidate "$destination"
/usr/bin/stat -f 'Installed mode=%Sp owner=%Su size=%z' "$destination"
printf 'Installed release SHA-256 %s\nRecovery: %s\nRunning sessions were not changed.\n' "$expected_sha" "$backup"
