#!/bin/bash
# Dev server for the rn frontend. dx defaults to :8080 — the port only comes
# from this flag, there is no Dioxus.toml key for it in dx 0.7.
cd "$(dirname "$0")"
exec dx serve --platform web --port 1790 "$@"
