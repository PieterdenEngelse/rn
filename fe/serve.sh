#!/bin/bash
# Dev server for the rn frontend. dx defaults to :8080 — the port only comes
# from this flag, there is no Dioxus.toml key for it in dx 0.7.
#
# $PORT overrides it, so parallel worktrees can each serve their own build
# without three dx servers fighting over :1790. Unset, it stays :1790, which is
# the port the backend's CORS default and the docs both name.
#
# rn-grid.service already supplies it, one port per pane — ~/ca 1791, ~/cb
# 1792, ~/cc 1793, and the ~/rn panes 1790, which are the four RN_CORS_ORIGIN
# allows. Nothing here reproduces that mapping: a second copy of it could only
# ever disagree with the first.
cd "$(dirname "$0")"

# The build directory is the part the environment gets wrong. That same service
# exports one CARGO_TARGET_DIR into every pane, so without this override each
# server writes crate `fe` over the others' output — the mismatched js/wasm
# pair that serves a blank page, and the reason a second dx serve used to be
# banned outright. ~/rn keeps the shared path, so its existing build is not
# orphaned and the native artifacts every worktree's cargo shares stay shared.
worktree="$(basename "$(cd .. && pwd)")"
if [ "$worktree" != "rn" ]; then
    export CARGO_TARGET_DIR="$HOME/.cache/rn-target-$worktree"
fi

echo "serving $worktree on http://127.0.0.1:${PORT:-1790}"
exec dx serve --platform web --port "${PORT:-1790}" "$@"
