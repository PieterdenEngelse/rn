#!/bin/bash
# Where a worktree's dev backend listens, and where its frontend looks for it.
#
# Sourced by fe/serve.sh and be/s so the rule lives once. Both need the same
# answer — a frontend pointed at a backend that is not there is a page of
# "backend unreachable", which looks like a broken build rather than a
# misconfigured pair.
#
# Derived from $PORT, which rn-grid.service exports one per pane, because that
# is already the single source of which worktree is which. Deriving it from the
# worktree's *name* instead would be a second copy of that mapping, and a second
# copy can only ever disagree with the first — see the commit that removed one.
#
#   pane      PORT   API    hooks
#   ~/rn      1790   3010   3011
#   ~/ca      1791   3020   3021
#   ~/cb      1792   3030   3031
#   ~/cc      1793   3040   3041
#
# Ten apart rather than one, so the API and its hooks listener never land on a
# neighbour's pair. The hooks port is the one a tunnel points at, and pointing a
# tunnel at another worktree's backend is not a mistake worth making reachable.
#
# BACKEND_PORT set in the environment wins outright: an install that has picked
# its own port is not something a dev script should second-guess.

_rn_fe_port="${PORT:-1790}"
_rn_slot=$(( _rn_fe_port - 1790 ))
# A port outside the grid's range means the pane was started by hand. Slot 0 is
# the honest answer there: the defaults every doc names, rather than a number
# derived from an offset nobody chose.
if [ "$_rn_slot" -lt 0 ] || [ "$_rn_slot" -gt 9 ]; then
    _rn_slot=0
fi

export BACKEND_PORT="${BACKEND_PORT:-$(( 3010 + _rn_slot * 10 ))}"
export BACKEND_HOOKS_PORT="${BACKEND_HOOKS_PORT:-$(( BACKEND_PORT + 1 ))}"
# What the frontend compiles in. fe reads this through option_env! at build
# time, since a wasm bundle has no environment to read at runtime.
export RN_API_BASE="${RN_API_BASE:-http://127.0.0.1:$BACKEND_PORT}"

# The pane's own frontend, as an origin the backend will answer. Without this a
# worktree's backend allows only :1790 — config.ts's default — and refuses the
# very frontend that just started beside it, with the failure the same comment
# warns about: "backend unreachable" while the backend is plainly running. Both
# spellings, because localhost and 127.0.0.1 are one server and two origins.
#
# Not for slot 0. `--env-file` does not override a variable already in the
# environment, so exporting this would quietly replace ~/rn/be/.env's list for
# the real install, and that file is the authority there.
if [ "$_rn_slot" -ne 0 ]; then
    export RN_CORS_ORIGIN="${RN_CORS_ORIGIN:-http://localhost:$_rn_fe_port,http://127.0.0.1:$_rn_fe_port}"
fi

unset _rn_fe_port _rn_slot
