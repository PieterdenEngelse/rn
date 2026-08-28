#!/bin/bash
# Dev server for the rn frontend. dx defaults to :8080 — the port only comes
# from this flag, there is no Dioxus.toml key for it in dx 0.7.
#
# One server per worktree, addressed by the worktree's own name.
#
# dx watches the directory it is run from and nothing else, so a session
# editing ~/cb sees no hot reload from a server running out of ~/rn: as far as
# that watcher is concerned the file never changed. Running one server per
# worktree is what makes every session's edits reload, and both things that
# would otherwise collide — the port and the build directory — are derived
# here rather than passed, so starting it is still just `./s`.
cd "$(dirname "$0")"

worktree="$(basename "$(cd .. && pwd)")"
case "$worktree" in
    rn) port_offset=0 ;;
    ca) port_offset=1 ;;
    cb) port_offset=2 ;;
    cc) port_offset=3 ;;
    # An unrecognised checkout gets the default port. Better a clear "address
    # already in use" than a silent second server on a port nothing expects.
    *)  port_offset=0 ;;
esac

# 1790–1793 are the four the backend already allows: RN_CORS_ORIGIN names each
# of them for both localhost and 127.0.0.1. A fifth worktree needs adding there
# too, or its fetches fail CORS with the page otherwise looking fine.
: "${PORT:=$((1790 + port_offset))}"

# The build directory has to differ per worktree, and the environment is
# actively against that: rn-grid.service sets CARGO_TARGET_DIR for every pane,
# so without this every server would write crate `fe` into one directory and
# overwrite the others' output — the mismatched js/wasm pair that serves a
# blank page. ~/rn keeps the original path so its existing build is not
# orphaned; the rest get one named after themselves.
if [ "$worktree" = "rn" ]; then
    export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/rn-target}"
else
    export CARGO_TARGET_DIR="$HOME/.cache/rn-target-$worktree"
fi

echo "serving $worktree on http://127.0.0.1:$PORT  (build dir $CARGO_TARGET_DIR)"
exec dx serve --platform web --port "$PORT" "$@"
