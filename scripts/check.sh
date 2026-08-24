#!/usr/bin/env bash
# Everything, in one command.
#
# This exists because a subset quietly became "the tests". The Rust crates and
# the Node backend have separate runners, and for a whole session only one of
# them was being run — nothing failed, nothing warned, and the launcher's suite
# simply was not being checked. A command that covers both is the fix; a
# reminder to remember is not.
#
# Windows twin: scripts/check.ps1. Change one, change the other.
set -euo pipefail

cd "$(dirname "$0")/.."
failed=()

step() {
    local name="$1"; shift
    printf '\n\033[1m== %s ==\033[0m\n' "$name"
    if "$@"; then
        printf '\033[32mok\033[0m  %s\n' "$name"
    else
        printf '\033[31mFAILED\033[0m  %s\n' "$name"
        failed+=("$name")
    fi
}

# Rust: one workspace, so this covers fe, launcher and shared together.
step "cargo test --workspace"   cargo test --workspace --quiet
step "cargo clippy"             cargo clippy --workspace --all-targets --quiet

# Node: its own runner, which is the half that went unchecked.
step "be: npm test"             npm --prefix be test
step "be: typecheck"            npm --prefix be run typecheck

printf '\n'
if [ ${#failed[@]} -eq 0 ]; then
    printf '\033[32mAll checks passed.\033[0m\n'
    exit 0
fi
printf '\033[31m%d check(s) failed:\033[0m\n' "${#failed[@]}"
printf '  - %s\n' "${failed[@]}"
exit 1
