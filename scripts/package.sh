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
#
# Then:  dist/rn/install.sh
#
# The page is the expensive step: a release wasm build of fe. From a cold
# cache that kind of build has run this machine out of memory with an editor
# open, so close heavy things first. Nothing else depends on it, so --no-web
# packages everything else in a few minutes.
#
# Linux x64 only (docs/packaging.md §9). There is still no package.ps1: on
# Windows, scripts/install.ps1 does this job inline against the checkout it
# sits in, which is the stopgap its own header explains.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$REPO/dist/rn"
WEB=1
SIG=1

log()  { printf '  %s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
warn() { printf '  ! %s\n' "$*"; }
die()  { printf '\nERROR: %s\n' "$*" >&2; exit 1; }
usage() { sed -n '3,12p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
    case $1 in
        --out)     OUT="$2"; shift 2 ;;
        --no-web)  WEB=0; shift ;;
        --no-sig)  SIG=0; shift ;;
        -h|--help) usage; exit 0 ;;
        *)         echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done
OUT="$(realpath -m "$OUT")"
case "$OUT" in
    / | "$HOME" | "$REPO" | "$REPO/be" | "$REPO/fe") die "refusing to build into $OUT, which gets deleted first" ;;
esac

# The bundled runtime ships without npm (§4 drops it on purpose), so the npm
# on PATH does the one build-time install, running under the bundled node.
command -v npm >/dev/null || die "npm is not on PATH; it builds app/node_modules"
[ "$WEB" = 0 ] || command -v dx >/dev/null || die "dx is not on PATH; it builds the page (or pass --no-web)"

# Where this worktree's cargo writes, the same rule check.sh and serve.sh use.
# shellcheck source=dev-target.sh
. "$REPO/scripts/dev-target.sh"

step "Output: $OUT"
rm -rf "$OUT"
mkdir -p "$OUT/app"

step "Launcher (release build)"
(cd "$REPO" && cargo build --release -p rn)
install -m 755 "$RN_TARGET_DIR/release/rn" "$OUT/rn"
log "rn  $(du -h "$OUT/rn" | cut -f1)"

step "Runtime"
if [ "$SIG" = 1 ]; then
    "$REPO/scripts/install-node.sh" --dest "$OUT/runtime" --require-sig
else
    warn "--no-sig: the runtime is checked by checksum only. Try it here; do not ship it."
    "$REPO/scripts/install-node.sh" --dest "$OUT/runtime"
fi

step "Backend (app/)"
cp -a "$REPO/be/src" "$OUT/app/src"
# runtime-params.json: the registry the launcher reads. .env.example: the
# settings page reads it beside .env to say which keys exist.
cp "$REPO/be/package.json" "$REPO/be/package-lock.json" \
   "$REPO/be/runtime-params.json" "$REPO/be/.env.example" "$OUT/app/"
# §3.4: npm ci at build time against the bundled runtime, and ship the tree,
# so the installed app never runs npm. --ignore-scripts because an install
# script is arbitrary code, and none of the dependencies needs one.
(cd "$OUT/app" && PATH="$OUT/runtime/bin:$PATH" \
    npm ci --omit=dev --ignore-scripts --no-audit --no-fund --loglevel=error)
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

cp "$REPO/scripts/install.sh" "$OUT/install.sh"
chmod 755 "$OUT/install.sh"
{
    echo "rn $(git -C "$REPO" describe --always --dirty), built $(date -u +%Y-%m-%dT%H:%MZ)"
    echo "node $(cat "$OUT/runtime/VERSION" 2>/dev/null || echo unknown)"
    echo "page $([ -d "$OUT/app/web" ] && echo included || echo "not included (--no-web)")"
    echo "runtime signature $([ "$SIG" = 1 ] && echo verified || echo "NOT verified (--no-sig)")"
} > "$OUT/BUILD"

step "Done: $(du -sh "$OUT" | cut -f1)"
# The splitting is the point: one argument when the page is there, none when
# it is not. Quoting it would hand du an empty argument to fail on.
# shellcheck disable=SC2046
(cd "$OUT" && du -sh rn runtime app/node_modules app/src $([ -d app/web ] && echo app/web) | sed 's/^/  /')
log "install:  $OUT/install.sh"
