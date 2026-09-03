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
worktree="$(basename "$PWD")"
if [ "$worktree" != "rn" ] && [ -z "${RN_CHECK_TARGET_DIR:-}" ]; then
    export CARGO_TARGET_DIR="$HOME/.cache/rn-target-$worktree"
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
