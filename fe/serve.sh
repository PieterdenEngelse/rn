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

port="${PORT:-1790}"

# One server per port, decided here rather than by whoever reads the pane. dx
# binds the port exclusively, so a second one on the same port dies with
# "Address already in use" — an error that names neither the holder nor the
# fact that the holder is doing its job. Worse, dx re-emits its status panel
# after every build, so a pane that has rebuilt twice already shows two boxes;
# add a failed second server's output and the pane reads like two servers when
# it has never been anything but one. Naming the holder makes that one line.
holder=$(ss -lptnH "sport = :$port" 2>/dev/null |
    grep -o 'pid=[0-9]*' | head -1 | cut -d= -f2)
if [ -n "$holder" ]; then
    what=$(ps -o comm= -p "$holder" 2>/dev/null)
    where=$(ps -o tty= -p "$holder" 2>/dev/null | tr -d ' ')
    since=$(ps -o lstart= -p "$holder" 2>/dev/null)
    [ "$where" = "?" ] && where="no tty"
    echo "serve.sh: :$port is already served by ${what:-an exited process} (pid $holder) on ${where:-no tty}, since $since"
    # Which advice is right depends on what holds it, and the wrong half of it
    # is worse than none: telling someone to press r at a python server sends
    # them looking for a dx that is not there.
    if [ "$what" = "dx" ]; then
        echo "serve.sh: that is a dev server already. Rebuild with r in its pane, stop it there with ctrl+c."
    else
        echo "serve.sh: that is not a dev server. Free the port, or serve this worktree elsewhere with PORT=."
    fi
    echo "serve.sh: nothing was started."
    exit 1
fi

echo "serving $worktree on http://127.0.0.1:$port"
exec dx serve --platform web --port "$port" "$@"
