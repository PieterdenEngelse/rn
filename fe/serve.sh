#!/bin/bash
# Dev server for the rn frontend. dx defaults to :8080 — the port only comes
# from this flag, there is no Dioxus.toml key for it in dx 0.7.
#
# $PORT overrides it, so parallel worktrees can each serve their own build
# without three dx servers fighting over :1790. Unset, it stays :1790, which is
# the port the backend's CORS default and the docs both name.
cd "$(dirname "$0")"
exec dx serve --platform web --port "${PORT:-1790}" "$@"
