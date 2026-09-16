#!/usr/bin/env bash
#
# Build a packaged rn: the tree docs/packaging.md §1 describes, with
# install.sh copied in beside it.
#
#   scripts/package.sh               dist/rn: launcher, runtime, backend, page
#   scripts/package.sh --out DIR     build somewhere other than dist/rn
#   scripts/package.sh --no-web      leave out the page (the wasm build, below)
#   scripts/package.sh --no-sig      runtime checked by checksum only: for
#                                    trying the package here, never for shipping
#   scripts/package.sh --target windows
#                                    dist/rn-win: the same tree for Windows,
#                                    cross-built from here (see below)
#
# Then:  dist/rn/install.sh
#
# The page is the expensive step: a release wasm build of fe. From a cold
# cache that kind of build has run this machine out of memory with an editor
# open, so close heavy things first. Nothing else depends on it, so --no-web
# packages everything else in a few minutes.
#
# Two targets, one build machine. Only two files in the tree below are
# platform-specific — the launcher and the Node binary — and both can be
# produced here: cargo-xwin links a real PE against the Microsoft CRT without
# leaving Linux, and install-node.sh takes --platform win-x64. Everything else
# (app/src, app/node_modules, app/web) is byte-identical on either target,
# because the backend has no native addons and the page is wasm. Verified: the
# only platform-gated packages in be/package-lock.json are the @typescript/*
# binaries, which are devDependencies and never reach --omit=dev.
#
# So there is still no package.ps1, and now there is less reason for one. A
# Windows package built here can be published like the Linux one, which is what
# lets scripts/install.ps1 -FromRelease install on a machine with no toolchain.
# Building on Windows instead remains possible; nothing here prevents it.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT=""
WEB=1
SIG=1
TARGET=linux

log()  { printf '  %s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
warn() { printf '  ! %s\n' "$*"; }
die()  { printf '\nERROR: %s\n' "$*" >&2; exit 1; }
usage() { sed -n '3,15p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
    case $1 in
        --out)     OUT="$2"; shift 2 ;;
        --no-web)  WEB=0; shift ;;
        --no-sig)  SIG=0; shift ;;
        --target)  TARGET="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *)         echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done
case "$TARGET" in
    linux)
        LAUNCHER=rn; NODE_PLATFORM=""; INSTALLER=install.sh
        DEFAULT_OUT="$REPO/dist/rn" ;;
    windows)
        LAUNCHER=rn.exe; NODE_PLATFORM=win-x64; INSTALLER=install.ps1
        DEFAULT_OUT="$REPO/dist/rn-win" ;;
    *) die "unknown --target $TARGET (linux or windows)" ;;
esac
OUT="${OUT:-$DEFAULT_OUT}"
OUT="$(realpath -m "$OUT")"
case "$OUT" in
    / | "$HOME" | "$REPO" | "$REPO/be" | "$REPO/fe") die "refusing to build into $OUT, which gets deleted first" ;;
esac

# The bundled runtime ships without npm (§4 drops it on purpose), so the npm
# on PATH does the one build-time install, running under the bundled node.
command -v npm >/dev/null || die "npm is not on PATH; it builds app/node_modules"
[ "$WEB" = 0 ] || command -v dx >/dev/null || die "dx is not on PATH; it builds the page (or pass --no-web)"
if [ "$TARGET" = windows ]; then
    command -v cargo-xwin >/dev/null \
        || die "cargo-xwin is not on PATH; it links the Windows launcher: cargo install cargo-xwin"
fi

# Where this worktree's cargo writes, the same rule check.sh and serve.sh use.
# shellcheck source=dev-target.sh
. "$REPO/scripts/dev-target.sh"

step "Output: $OUT"
rm -rf "$OUT"
mkdir -p "$OUT/app"

step "Launcher (release build, $TARGET)"
if [ "$TARGET" = windows ]; then
    # MSVC ABI rather than gnu: it is what a Windows user's tooling expects,
    # and cargo-xwin fetches the CRT and SDK headers itself, so this needs no
    # mingw and no root. The launcher's own Windows code paths are compiled
    # for the first time by this command — on Linux they are only parsed.
    (cd "$REPO" && cargo xwin build --release -p rn --target x86_64-pc-windows-msvc)
    src_exe="$RN_TARGET_DIR/x86_64-pc-windows-msvc/release/rn.exe"
    head -c2 "$src_exe" | grep -q '^MZ' || die "$src_exe is not a PE executable"
else
    (cd "$REPO" && cargo build --release -p rn)
    src_exe="$RN_TARGET_DIR/release/rn"
fi
install -m 755 "$src_exe" "$OUT/$LAUNCHER"
log "$LAUNCHER  $(du -h "$OUT/$LAUNCHER" | cut -f1)"

step "Runtime"
node_args=(--dest "$OUT/runtime")
[ -n "$NODE_PLATFORM" ] && node_args+=(--platform "$NODE_PLATFORM")
if [ "$SIG" = 1 ]; then
    node_args+=(--require-sig)
else
    warn "--no-sig: the runtime is checked by checksum only. Try it here; do not ship it."
fi
"$REPO/scripts/install-node.sh" "${node_args[@]}"

step "Backend (app/)"
cp -a "$REPO/be/src" "$OUT/app/src"
# runtime-params.json: the registry the launcher reads. .env.example: the
# settings page reads it beside .env to say which keys exist.
cp "$REPO/be/package.json" "$REPO/be/package-lock.json" \
   "$REPO/be/runtime-params.json" "$REPO/be/.env.example" "$OUT/app/"
# §3.4: npm ci at build time against the bundled runtime, and ship the tree,
# so the installed app never runs npm. --ignore-scripts because an install
# script is arbitrary code, and none of the dependencies needs one.
# The PATH prefix puts the bundled runtime in front for a linux package, so
# npm runs under the Node that ships. For a windows one the directory holds
# node.exe, which this machine cannot run and which PATH lookup ignores, so
# npm falls through to the ambient Node — which changes nothing about the tree
# it writes: no native addons, --ignore-scripts, and npm only unpacks tarballs.
(cd "$OUT/app" && PATH="$OUT/runtime/bin:$PATH" \
    npm ci --omit=dev --ignore-scripts --no-audit --no-fund --loglevel=error)
# node_modules/.bin holds Unix symlinks — npm writes .cmd and .ps1 shims
# there on Windows instead — and a symlink in a zip either arrives as a broken
# stub or refuses to extract. Nothing in the installed app runs a CLI from it:
# the launcher spawns node against app/src/server.ts and that is the whole of
# it. So the directory is dropped from a windows package rather than shipped
# meaning nothing. Verified before removing: the only entry is pino's
# pretty-printer, which no file under be/src references.
if [ "$TARGET" = windows ] && [ -d "$OUT/app/node_modules/.bin" ]; then
    rm -rf "$OUT/app/node_modules/.bin"
    log "dropped app/node_modules/.bin (Unix symlinks, unused, unzippable on Windows)"
fi
if [ "$TARGET" = windows ] && [ -n "$(find "$OUT" -type l)" ]; then
    die "the windows package still holds symlinks: $(find "$OUT" -type l | head -3 | tr '\n' ' ')"
fi
log "app/node_modules  $(du -sh "$OUT/app/node_modules" | cut -f1)"

if [ "$WEB" = 1 ]; then
    step "Page (release wasm build of fe; memory-hungry from cold)"
    (cd "$REPO/fe" && npm run --silent css:build)
    # RN_API_BASE is read at compile time, and cargo does not rebuild when an
    # environment variable changes. Touching the one file that reads it makes
    # sure the empty base is what gets compiled, not a cached dev address.
    touch "$REPO/fe/src/api/client.rs"
    # dx never prunes its release output. Every bundle's hashed js and wasm
    # stay in <target>/dx/fe/release/web/public, and the next bundle copies
    # all of them out: on 2026-09-11 a second build shipped the old 4.1 MB
    # wasm beside the new 2.5 MB one. Starting from an empty public/ keeps the
    # package to what index.html loads. The dev server builds elsewhere
    # (its own profile), so this touches nothing it serves.
    rm -rf "$RN_TARGET_DIR/dx/fe/release/web/public"
    tmp=$(mktemp -d)
    dxlog=$(mktemp)
    trap 'rm -rf "$tmp" "$dxlog"' EXIT
    # --debug-symbols=false: dx defaults it to true even for --release, and
    # the DWARF that leaves in the wasm is a version binaryen cannot read. So
    # wasm-opt aborted ("compile unit size was incorrect", SIGABRT) and the
    # page shipped unoptimised. dx carries on after that with exit status 0,
    # which is why its output is kept and read below.
    (cd "$REPO/fe" && RN_API_BASE="" dx bundle --web --release --debug-symbols=false --out-dir "$tmp") 2>&1 \
        | tee "$dxlog"
    if grep -q 'wasm-opt failed' "$dxlog"; then
        warn "wasm-opt failed, so the page ships unoptimised: larger, still working. See the dx output above."
    fi
    index=$(find "$tmp" -name index.html -print -quit)
    [ -n "$index" ] || die "dx bundle wrote no index.html under $tmp"
    cp -a "$(dirname "$index")" "$OUT/app/web"
    nwasm=$(find "$OUT/app/web" -name '*.wasm' | wc -l)
    [ "$nwasm" = 1 ] || die "app/web holds $nwasm wasm files, expected 1: leftovers from an earlier bundle"
    # The failure this catches is silent otherwise: a page that loads from the
    # install and then calls a development backend on :3010 for every request.
    if grep -rqaF --include='*.wasm' 'http://127.0.0.1:3010' "$OUT/app/web"; then
        die "the page was compiled with the development API address; clean fe's release build and rerun"
    fi
    log "app/web  $(du -sh "$OUT/app/web" | cut -f1)"
else
    warn "--no-web: no page in this package. The installed backend runs, and says at boot that it has none."
fi

cp "$REPO/scripts/$INSTALLER" "$OUT/$INSTALLER"
chmod 755 "$OUT/$INSTALLER"
# The graphical front end travels with the package it installs, so a downloaded
# tarball offers the same two ways in as the repository does. It is a face over
# install.sh and finds it by sitting beside it, which is exactly the layout
# here. Linux only, because there is nothing on the Windows side for it to be
# the twin of — install-gui.sh's header says why.
if [ "$TARGET" = linux ]; then
    cp "$REPO/scripts/install-gui.sh" "$OUT/install-gui.sh"
    chmod 755 "$OUT/install-gui.sh"
fi
{
    echo "rn $(git -C "$REPO" describe --always --dirty), built $(date -u +%Y-%m-%dT%H:%MZ) for $TARGET"
    echo "node $(cat "$OUT/runtime/VERSION" 2>/dev/null || echo unknown)"
    echo "page $([ -d "$OUT/app/web" ] && echo included || echo "not included (--no-web)")"
    echo "runtime signature $([ "$SIG" = 1 ] && echo verified || echo "NOT verified (--no-sig)")"
} > "$OUT/BUILD"

step "Done: $(du -sh "$OUT" | cut -f1)"
# The splitting is the point: one argument when the page is there, none when
# it is not. Quoting it would hand du an empty argument to fail on.
# shellcheck disable=SC2046
(cd "$OUT" && du -sh "$LAUNCHER" runtime app/node_modules app/src $([ -d app/web ] && echo app/web) | sed 's/^/  /')
log "install:  $OUT/$INSTALLER"
# An if, not `[ ] && log`: as the last statement in the file that would make
# the script exit 1 on every linux build, since the status of the last command
# is the status of the script.
if [ "$TARGET" = windows ]; then
    log "          (on the Windows machine: .\\install.ps1 -SkipBuild -Out .)"
fi
