#!/usr/bin/env bash
#
# Install a packaged rn for the current user: the other half of
# scripts/package.sh, which copies this file into the package it builds.
#
#   dist/rn/install.sh                 install from the package it sits in
#   dist/rn/install.sh --prefix DIR    somewhere other than ~/.local/share/rn
#   dist/rn/install.sh --no-service    files only: no systemd unit, no menu entry
#   dist/rn/install.sh --no-start      install and enable, but do not start
#   ~/.local/share/rn/install.sh --uninstall
#                                      remove it again; ~/.config/rn is kept
#
# Per-user and without root, as docs/packaging.md §8 prefers. The install
# directory is replaced whole on every upgrade, so nothing the user owns lives
# in it: settings, state and credentials are in ~/.config/rn, which this script
# never touches. The one file carried across an upgrade is app/.env, because
# the launcher reads its ports from there.
#
# Linux only, like the package (docs/packaging.md §9). There is deliberately no
# install.ps1: a twin for a platform nobody here can build or run would look
# maintained without being so, which CLAUDE.md calls worse than no twin.
set -euo pipefail

PKG="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${XDG_DATA_HOME:-$HOME/.local/share}/rn"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
UNIT=rn.service
SERVICE=1
START=1
UNINSTALL=0

log()  { printf '  %s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
warn() { printf '  ! %s\n' "$*"; }
die()  { printf '\nERROR: %s\n' "$*" >&2; exit 1; }
usage() { sed -n '3,12p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
    case $1 in
        --prefix)     PREFIX="$2"; shift 2 ;;
        --no-service) SERVICE=0; START=0; shift ;;
        --no-start)   START=0; shift ;;
        --uninstall)  UNINSTALL=1; shift ;;
        -h|--help)    usage; exit 0 ;;
        *)            echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done
PREFIX="$(realpath -m "$PREFIX")"

# The install directory gets deleted and replaced, so be sure it is ours.
case "$PREFIX" in
    / | "$HOME" | "$(realpath -m "$HOME/..")") die "refusing to use $PREFIX as the install directory" ;;
esac
if [ -d "$PREFIX" ] && [ ! -e "$PREFIX/rn" ] && [ -n "$(ls -A "$PREFIX")" ]; then
    die "$PREFIX exists and is not an rn install; not replacing it"
fi

have_systemd() { command -v systemctl >/dev/null && systemctl --user show-environment >/dev/null 2>&1; }

# Only ever stop the installed service by its unit. `rn --stop` would be
# wrong here: the launcher's pidfile is per user, not per install, so on a
# machine that also runs a development backend it names that one.
stop_service() {
    if have_systemd && systemctl --user is-active -q "$UNIT"; then
        log "stopping $UNIT"
        systemctl --user stop "$UNIT"
    fi
}

# BACKEND_PORT from app/.env, as the launcher reads it; 3010 otherwise.
api_port() {
    local p
    p=$(sed -n 's/^[[:space:]]*BACKEND_PORT[[:space:]]*=[[:space:]]*["'\'']\{0,1\}\([0-9][0-9]*\).*/\1/p' \
        "$PREFIX/app/.env" 2>/dev/null | tail -1)
    echo "${p:-3010}"
}

if [ "$UNINSTALL" = 1 ]; then
    step "Uninstalling $PREFIX"
    stop_service
    if have_systemd && [ -f "$UNIT_DIR/$UNIT" ]; then
        systemctl --user disable -q "$UNIT" 2>/dev/null || true
    fi
    rm -f "$UNIT_DIR/$UNIT" "$APPS_DIR/rn.desktop"
    have_systemd && systemctl --user daemon-reload
    [ -e "$PREFIX/rn" ] && rm -rf "$PREFIX"
    log "removed the install, $UNIT and the menu entry"
    log "kept ~/.config/rn (settings, state, credentials); delete it by hand to forget everything"
    exit 0
fi

step "Checking the package"
for f in rn runtime/bin/node app/src/server.ts app/runtime-params.json app/node_modules; do
    [ -e "$PKG/$f" ] || die "not a complete package: $PKG/$f is missing (build one with scripts/package.sh)"
done
[ "$PKG" = "$PREFIX" ] && die "this package already is the install at $PREFIX"
log "package:  $PKG"
log "node:     $(cat "$PKG/runtime/VERSION" 2>/dev/null || "$PKG/runtime/bin/node" --version)"
[ -f "$PKG/BUILD" ] && log "build:    $(head -1 "$PKG/BUILD")"
if [ -f "$PKG/app/web/index.html" ]; then
    log "page:     app/web, served by the backend on the API's own port"
else
    warn "this package has no app/web: the backend will run, but has no page of its own to serve"
fi

step "Installing to $PREFIX"
stop_service
new="$PREFIX.new.$$"
old="$PREFIX.old.$$"
rm -rf "$new"
mkdir -p "$(dirname "$PREFIX")"
cp -a "$PKG" "$new"
if [ -f "$PREFIX/app/.env" ]; then
    cp -a "$PREFIX/app/.env" "$new/app/.env"
    log "kept app/.env from the previous install"
fi
# Swap rather than copy over: a half-copied tree is never the live one, and
# the old tree is only deleted once the new one is in its place.
[ -e "$PREFIX" ] && mv "$PREFIX" "$old"
mv "$new" "$PREFIX"
rm -rf "$old"
log "installed, $(du -sh "$PREFIX" | cut -f1)"

port=$(api_port)
url="http://127.0.0.1:$port/"

if [ "$SERVICE" = 1 ]; then
    step "Service and menu entry"
    have_systemd || die "no systemd user session here; rerun with --no-service and start $PREFIX/rn yourself"
    mkdir -p "$UNIT_DIR" "$APPS_DIR"
    cat > "$UNIT_DIR/$UNIT" <<EOF
# Written by $PREFIX/install.sh; rewritten on every install.
[Unit]
Description=rn (installed: the launcher and the Node process it supervises)

[Service]
# The launcher runs in the foreground and supervises Node itself, so systemd
# watches the launcher and the launcher watches Node.
Type=simple
WorkingDirectory=$PREFIX/app
ExecStart=$PREFIX/rn
ExecStop=$PREFIX/rn --stop
# A deliberate stop exits 0 and stays down; a crash comes back.
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
EOF
    cat > "$APPS_DIR/rn.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=rn
Comment=Open rn; the backend itself runs as $UNIT
Exec=xdg-open $url
Terminal=false
Categories=Development;Utility;
EOF
    systemctl --user daemon-reload
    systemctl --user enable -q "$UNIT"
    log "enabled $UNIT; the menu entry opens $url"
fi

if [ "$START" = 1 ]; then
    step "Starting"
    holder=$(ss -ltnpH "sport = :$port" 2>/dev/null | head -1)
    if [ -n "$holder" ]; then
        warn "port $port is already taken, so $UNIT was not started:"
        warn "  $holder"
        warn "a development backend (rn-backend.service) uses the same ports and the same ~/.config/rn."
        warn "stop that first, or set BACKEND_PORT in $PREFIX/app/.env, then: systemctl --user start $UNIT"
    else
        systemctl --user start "$UNIT"
        up=0
        for _ in $(seq 1 30); do
            if curl -fsS "${url}api/health" >/dev/null 2>&1; then up=1; break; fi
            sleep 0.5
        done
        if [ "$up" = 1 ]; then
            log "running: $url"
        else
            warn "started, but ${url}api/health is not answering yet; see: journalctl --user -u $UNIT"
        fi
    fi
fi

step "Done"
log "launcher:  $PREFIX/rn  (--status, --print-env)"
log "settings:  ~/.config/rn  (never touched by install or uninstall)"
log "remove:    $PREFIX/install.sh --uninstall"
