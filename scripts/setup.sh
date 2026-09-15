#!/usr/bin/env bash
#
# Development setup for be/ — the executable version of docs/setup-js.md.
#
# If this script and that document ever disagree, the script is right and the
# document is a bug. Run it as often as you like; every step is idempotent.
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BE="$REPO_ROOT/be"

log()  { printf '  %s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
warn() { printf '  ! %s\n' "$*"; }
die()  { printf '\nERROR: %s\n' "$*" >&2; exit 1; }

cd "$BE"
WANT="$(tr -d '[:space:]' < .nvmrc)"

# --- 1. the Node you develop with -------------------------------------------
step "Node version"
HAVE="$(node --version 2>/dev/null || echo none)"
log "want (be/.nvmrc): $WANT"
log "have (on PATH):   $HAVE"
if [[ "$HAVE" != "$WANT" ]]; then
    warn "Mismatch. Dev/production drift is the classic 'works on my machine'."
    warn "Fix with:  nvm install $WANT && nvm use $WANT"
    warn "Continuing — the bundled runtime below is the one that ships."
fi

# --- 2. the Node that ships --------------------------------------------------
step "Private runtime (be/runtime)"
"$REPO_ROOT/scripts/install-node.sh" --dest "$BE/runtime"

# --- 3. dependencies ---------------------------------------------------------
step "Dependencies"
if [[ -f package-lock.json ]]; then
    log "npm ci (exact lockfile)"
    npm ci
else
    log "no lockfile yet — npm install, which creates one. Commit it."
    npm install
fi

# --- 4. local configuration --------------------------------------------------
step "Configuration"
if [[ -f .env ]]; then
    log ".env exists — left untouched"
else
    cp .env.example .env
    log "created .env from .env.example"
fi
missing=""
while IFS= read -r key; do
    grep -q "^$key=" .env || missing="$missing $key"
done < <(grep -oE '^[A-Z_][A-Z0-9_]*=' .env.example | tr -d '=')
if [[ -n "$missing" ]]; then
    warn "keys in .env.example missing from .env:$missing"
fi

# --- 5. prove it works -------------------------------------------------------
step "Verifying"
log "typecheck"; npm run --silent typecheck
# Not `&& pass || "(no tests yet)"`: every non-zero exit took that branch, so
# a suite that genuinely failed reported the one thing that reads as fine. It
# was true when be/ had no tests and stopped being true the day it got some.
log "tests"
if npm test --silent >/dev/null 2>&1; then
    log "  pass"
else
    log "  FAILED — see: cd be && npm test"
fi
log "run against the bundled runtime"
./runtime/bin/node --env-file-if-exists=.env src/main.ts | sed 's/^/    /'

step "Ready"
log "npm run dev            watch mode, ambient node"
log "npm run start:sealed   run against the bundled runtime (what users get)"
