#!/usr/bin/env bash
#
# Install a private Node runtime.
#
# The same script serves both purposes, which is the point: what a developer
# installs and what ships to a user must not drift apart.
#
#   scripts/install-node.sh                      -> be/runtime      (development)
#   scripts/install-node.sh --dest dist/runtime  -> packaged output (release)
#
# The version comes from be/.nvmrc — the single source of truth. Nothing here
# hardcodes it.
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION_FILE="$REPO_ROOT/be/.nvmrc"

DEST="$REPO_ROOT/be/runtime"
PLATFORM=""
VERSION=""
REQUIRE_SIG=0
FORCE=0
CACHE="${RN_CACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/rn/node}"

usage() {
    cat <<'USAGE'
Usage: install-node.sh [options]

  --dest DIR       where to install (default: be/runtime)
  --version VER    Node version, e.g. v24.19.0 (default: read from be/.nvmrc)
  --platform P     e.g. linux-x64, darwin-arm64 (default: detected)
  --require-sig    fail unless the GPG signature verifies. Use for releases.
  --force          reinstall even if the correct version is already present
  -h, --help       this text

The SHA-256 checksum is ALWAYS verified and a mismatch is always fatal.
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dest)        DEST="$2"; shift 2 ;;
        --version)     VERSION="$2"; shift 2 ;;
        --platform)    PLATFORM="$2"; shift 2 ;;
        --require-sig) REQUIRE_SIG=1; shift ;;
        --force)       FORCE=1; shift ;;
        -h|--help)     usage; exit 0 ;;
        *)             echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

log()  { printf '  %s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
die()  { printf '\nERROR: %s\n' "$*" >&2; exit 1; }

# --- version: one source of truth -------------------------------------------
if [[ -z "$VERSION" ]]; then
    [[ -f "$VERSION_FILE" ]] || die "no --version given and $VERSION_FILE is missing"
    VERSION="$(tr -d '[:space:]' < "$VERSION_FILE")"
fi
[[ "$VERSION" == v* ]] || VERSION="v$VERSION"

# --- platform ---------------------------------------------------------------
if [[ -z "$PLATFORM" ]]; then
    case "$(uname -s)" in
        Linux)  os=linux ;;
        Darwin) os=darwin ;;
        *)      die "unsupported OS $(uname -s). Windows uses scripts/install-node.ps1" ;;
    esac
    case "$(uname -m)" in
        x86_64|amd64)  arch=x64 ;;
        aarch64|arm64) arch=arm64 ;;
        *)             die "unsupported architecture $(uname -m)" ;;
    esac
    PLATFORM="$os-$arch"
fi

# Windows ships a zip laid out flat — node.exe at the root, no bin/ — where
# every other platform ships a tar.xz with bin/node inside. Both are normalised
# to $DEST/bin/<exe> below, so the launcher's path is the same everywhere
# (launcher/src/layout.rs resolves runtime/bin/node.exe there, runtime/bin/node
# here). install-node.ps1 does the same normalisation on the machine itself.
case "$PLATFORM" in
    win-*) IS_WIN=1; TARBALL="node-$VERSION-$PLATFORM.zip";   NODE_EXE=node.exe ;;
    *)     IS_WIN=0; TARBALL="node-$VERSION-$PLATFORM.tar.xz"; NODE_EXE=node ;;
esac
BASE_URL="https://nodejs.org/dist/$VERSION"

step "Node $VERSION for $PLATFORM -> $DEST"

# --- already installed? -----------------------------------------------------
if [[ $FORCE -eq 0 && -e "$DEST/bin/$NODE_EXE" ]]; then
    # A cross-built runtime cannot be asked its version — this machine cannot
    # execute it — so the VERSION file written beside it answers instead. It is
    # written last, after the binary is in place, so its presence means the
    # install completed rather than merely started.
    if [[ $IS_WIN -eq 1 ]]; then
        have="$(cat "$DEST/VERSION" 2>/dev/null || true)"
    else
        have="$("$DEST/bin/$NODE_EXE" --version 2>/dev/null || true)"
    fi
    if [[ "$have" == "$VERSION" ]]; then
        log "already installed ($have) — nothing to do. Use --force to reinstall."
        exit 0
    fi
    log "replacing ${have:-an unknown version} with $VERSION"
fi

for tool in curl sha256sum; do
    command -v "$tool" >/dev/null || die "required tool not found: $tool"
done
if [[ $IS_WIN -eq 1 ]]; then
    command -v unzip >/dev/null || die "required tool not found: unzip (needed for a win-* runtime)"
else
    for tool in tar xz; do
        command -v "$tool" >/dev/null || die "required tool not found: $tool"
    done
fi

# --- download (cached) ------------------------------------------------------
mkdir -p "$CACHE"
step "Downloading"
cd "$CACHE"
for f in "$TARBALL" SHASUMS256.txt SHASUMS256.txt.sig; do
    if [[ -s "$f" && "$f" == "$TARBALL" ]]; then
        log "cached: $f"
    else
        log "fetch:  $f"
        curl -fSL --retry 3 --no-progress-meter -o "$f" "$BASE_URL/$f" \
            || die "download failed: $BASE_URL/$f"
    fi
done

# --- verify: checksum is mandatory, signature is opt-in ---------------------
step "Verifying"
if grep " $TARBALL\$" SHASUMS256.txt | sha256sum -c - >/dev/null 2>&1; then
    log "sha256 OK"
else
    rm -f "$TARBALL"   # never leave a bad artifact in the cache
    die "SHA-256 mismatch for $TARBALL. Removed it; re-run to download again."
fi

# SHASUMS256.txt.sig is the detached signature over the very file the checksum
# above was read from. Until 2026-09-11 this verified SHASUMS256.txt.asc
# instead, which is the same list *clearsigned* (text and signature in one
# file), and gpg refuses that as a detached signature ("Packet type 63 not
# allowed"). So the check failed with every key imported, --require-sig could
# never pass, and the message below blamed missing keys.
sig_status=""
if command -v gpg >/dev/null; then
    sig_status="$(gpg --batch --status-fd 1 --verify SHASUMS256.txt.sig SHASUMS256.txt 2>/dev/null || true)"
fi
# VALIDSIG's last field is the primary key's fingerprint: who signed, shown
# rather than just "OK", so a release signed by an unexpected key is visible.
signer="$(awk '$2 == "VALIDSIG" { print $NF; exit }' <<< "$sig_status")"
if grep -q '^\[GNUPG:\] GOODSIG ' <<< "$sig_status" && [[ -n "$signer" ]]; then
    log "gpg signature OK (release key $signer)"
elif [[ $REQUIRE_SIG -eq 1 ]]; then
    die "signature verification failed: no gpg, a bad signature, or no Node release key in your keyring.
       Import the keys (docs/packaging.md §4 says how), then re-run.
       A checksum alone only proves the file matches a list that could itself be swapped."
else
    log "gpg signature NOT verified (no gpg, or no Node release key in the keyring)."
    log "Fine for development; use --require-sig for anything you ship."
fi

# --- install ----------------------------------------------------------------
step "Installing"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
if [[ $IS_WIN -eq 1 ]]; then
    unzip -q "$TARBALL" -d "$workdir"
else
    tar -xJf "$TARBALL" -C "$workdir"
fi
src="$workdir/node-$VERSION-$PLATFORM"

rm -rf "$DEST"
mkdir -p "$DEST/bin"
if [[ $IS_WIN -eq 1 ]]; then
    cp "$src/node.exe" "$DEST/bin/node.exe"
else
    cp "$src/bin/node" "$DEST/bin/node"
fi
cp "$src/LICENSE"  "$DEST/LICENSE"      # MIT — required when redistributing

# Dropped on purpose: include/ (C++ headers, ~57 MB), lib/node_modules/ (npm,
# ~13 MB), share/ (man pages). The installed app never compiles or runs npm.

# No strip on Windows: the official node.exe ships without separate debug
# symbols, and a host strip has no business rewriting a PE anyway.
if [[ $IS_WIN -eq 0 ]] && command -v strip >/dev/null; then
    before=$(stat -c%s "$DEST/bin/node")
    strip "$DEST/bin/node" || true
    after=$(stat -c%s "$DEST/bin/node")
    log "stripped: $((before/1048576)) MB -> $((after/1048576)) MB"
fi

# --- prove it is what it claims ---------------------------------------------
if [[ $IS_WIN -eq 1 ]]; then
    # It cannot be run here, so check the two things that can be checked: it is
    # a PE executable rather than an error page or an empty file, and it is big
    # enough to be a runtime. Running it is the installer's first act on the
    # machine itself, and install.ps1 fails loudly there if it is not.
    head -c2 "$DEST/bin/node.exe" | grep -q '^MZ' \
        || die "$DEST/bin/node.exe is not a PE executable"
    size=$(stat -c%s "$DEST/bin/node.exe")
    [[ $size -gt 20000000 ]] || die "node.exe is only $size bytes; that is not a runtime"
    got="$VERSION"
else
    got="$("$DEST/bin/$NODE_EXE" --version)"
    [[ "$got" == "$VERSION" ]] || die "installed binary reports $got, expected $VERSION"
fi

# Written last, so its presence means a finished install — the check above
# relies on that for a runtime this machine cannot execute.
printf '%s\n' "$VERSION" > "$DEST/VERSION"

step "Done"
log "$DEST/bin/$NODE_EXE  ($got)"
