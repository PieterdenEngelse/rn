#!/bin/bash
# Where this worktree's cargo output goes — and therefore where a binary it
# built is found.
#
# Sourced by fe/serve.sh, which needs cargo to *write* somewhere private, and by
# be/s, which needs to *read* what that wrote. Those were two halves of one rule
# living in one of the two files: serve.sh set the directory, be/s looked in
# `<worktree>/target` regardless, and so `be/s --status` in any worktree but
# ~/rn failed with a bare "No such file or directory" naming a path nothing had
# ever written to.
#
# The rule itself is forced by rn-grid.service, which exports one shared
# CARGO_TARGET_DIR into every pane. Without an override each pane's cargo writes
# crate `fe` over its neighbours' output — the mismatched js/wasm pair that
# serves a blank page. ~/rn keeps the shared path so its existing build is not
# orphaned and the native artifacts every worktree shares stay shared.
#
# Not folded into dev-ports.sh. That file derives ports from $PORT and says why
# deriving them from the worktree's name would be a second copy of the grid's
# mapping; this is the opposite — a rule that is *only* about the name, and has
# no business being read by be/d, which runs no cargo at all.
_rn_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
_rn_worktree="$(basename "$_rn_root")"
_rn_ambient="${CARGO_TARGET_DIR:-}"

if [ "$_rn_worktree" != "rn" ]; then
    export CARGO_TARGET_DIR="$HOME/.cache/rn-target-$_rn_worktree"
fi

# Where cargo writes from here on.
export RN_TARGET_DIR="${CARGO_TARGET_DIR:-$_rn_root/target}"
export RN_WORKTREE_ROOT="$_rn_root"

# Where a build might already be, most specific first — which is a different
# question, and answering it with RN_TARGET_DIR alone sends people to build
# something they have already built. All three of these hold a launcher on the
# machine this was written on:
#
#   - the directory chosen above, which is what fe/serve.sh builds into;
#   - whatever the pane already had, since rn-grid.service exports one shared
#     CARGO_TARGET_DIR and a plain `cargo build` in a pane lands there, not in
#     the per-worktree directory this file picks;
#   - the workspace default, for a build made before that variable existed.
#
# Colon-separated, like PATH, and read the same way.
RN_TARGET_SEARCH="$RN_TARGET_DIR"
if [ -n "$_rn_ambient" ] && [ "$_rn_ambient" != "$RN_TARGET_DIR" ]; then
    RN_TARGET_SEARCH="$RN_TARGET_SEARCH:$_rn_ambient"
fi
if [ "$_rn_root/target" != "$RN_TARGET_DIR" ]; then
    RN_TARGET_SEARCH="$RN_TARGET_SEARCH:$_rn_root/target"
fi
export RN_TARGET_SEARCH

unset _rn_root _rn_worktree _rn_ambient
