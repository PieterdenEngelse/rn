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

# **One target directory per worktree, for the reason fe/serve.sh has one.**
#
# rn-grid exports a single CARGO_TARGET_DIR into every pane, so without this
# every worktree's cargo writes and reads one directory. serve.sh already
# overrides it for dx; nothing did for a bare cargo, and the failure is worse
# here than a mismatched js/wasm pair: `cargo test --workspace` in one worktree
# links against whatever `shared` a peer happened to build last. That surfaces
# as this worktree's own frontend failing to resolve types it plainly defines —
#
#     error[E0432]: unresolved import `shared::webhooks`
#
# — with nothing pointing at the neighbour that caused it. It failed a landing
# twice, passed on a retry when the timing was kind, and read as flakiness both
# times. ~/rn keeps the shared path, exactly as serve.sh leaves it, so the
# user's own build is not orphaned.
#
# **The Windows twin deliberately does not have this**, which is a divergence
# from "change one, change the other" and so is written down rather than left to
# look like drift: rn-grid is a tmux layout on this machine, there is no pane
# exporting a shared CARGO_TARGET_DIR on Windows, and copying the workaround
# there would be inventing a cache path to solve a collision that cannot happen.
#
# The path itself is not written here. It was, and this file was the third copy
# of it after fe/serve.sh and be/s — which is how the first two managed to
# disagree for as long as they did, one setting the directory and the other
# reading somewhere else entirely. scripts/dev-target.sh owns it now.
#
# RN_CHECK_TARGET_DIR opts out, and its *value* is deliberately not read:
# setting it to anything at all leaves CARGO_TARGET_DIR exactly as the
# environment had it, which is what a CI runner or a bisect wants. The name
# reads like it should name a directory and does not — kept as it is because
# honouring the value would turn `RN_CHECK_TARGET_DIR=1`, the way anyone would
# have written an opt-out, into a target directory called `1`.
if [ -z "${RN_CHECK_TARGET_DIR:-}" ]; then
    # shellcheck source=./dev-target.sh
    . scripts/dev-target.sh
fi

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
