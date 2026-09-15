#!/usr/bin/env bash
#
# Publish a packaged rn as a GitHub release, so a machine with no Rust, Node
# or dx can install it with one command:
#
#   scripts/release.sh                 build this checkout and publish v<version>
#   scripts/release.sh --tag v0.1.1    a tag of your choosing
#   scripts/release.sh --from dist/rn  publish a package already built
#   scripts/release.sh --no-windows    Linux asset only (no cargo-xwin needed)
#   scripts/release.sh --from-windows dist/rn-win
#                                      use a Windows package already built
#   scripts/release.sh --draft         publish as a draft, to look at first
#
# Then, on any machine with gh signed in:
#
#   scripts/install.sh --from-release
#
# or on Windows, with nothing installed at all:
#
#   .\install.ps1 -FromRelease
#
# The version comes from launcher/Cargo.toml, the one crate that is the app
# itself. The tree must be clean: a release nobody can rebuild from a commit
# is not a release, and the BUILD file inside the package records the commit
# it came from.
#
# Both platforms, from this one machine: package.sh --target windows
# cross-builds the Windows tree here (docs/packaging.md §9), so a release
# carries rn-linux-x64.tar.gz and rn-windows-x64.zip without a Windows box
# existing anywhere.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
REPO=PieterdenEngelse/rn
PKG=""
WIN_PKG=""
TAG=""
DRAFT=()
ALLOW_DIRTY=0
WINDOWS=1

log()  { printf '  %s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
die()  { printf '\nERROR: %s\n' "$*" >&2; exit 1; }
usage() { sed -n '3,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
    case $1 in
        --tag)         TAG="$2"; shift 2 ;;
        --from)        PKG="$(realpath -m "$2")"; shift 2 ;;
        --from-windows) WIN_PKG="$(realpath -m "$2")"; shift 2 ;;
        --no-windows)  WINDOWS=0; shift ;;
        --draft)       DRAFT=(--draft); shift ;;
        --repo)        REPO="$2"; shift 2 ;;
        --allow-dirty) ALLOW_DIRTY=1; shift ;;
        -h|--help)     usage; exit 0 ;;
        *)             echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

command -v gh >/dev/null || die "gh is not installed"
gh auth status >/dev/null 2>&1 || die "gh is not signed in: run gh auth login"
if [ "$ALLOW_DIRTY" = 0 ] && [ -n "$(git status --porcelain)" ]; then
    die "the tree is dirty; commit first, or pass --allow-dirty for a test release"
fi

version=$(sed -n 's/^version = "\(.*\)"/\1/p' launcher/Cargo.toml | head -1)
[ -n "$version" ] || die "no version in launcher/Cargo.toml"
TAG=${TAG:-v$version}
commit=$(git rev-parse HEAD)
gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1 \
    && die "$TAG already exists on $REPO; pass --tag for a different one"

if [ -z "$PKG" ]; then
    step "Building the package"
    ./scripts/package.sh
    PKG="$REPO_ROOT/dist/rn"
fi

# The Windows package is cross-built from here, so a release can carry both
# without a second machine. Skipped with --no-windows, because the launcher
# needs cargo-xwin and a release should not be blocked on a tool that is only
# needed for the other half of it.
if [ "$WINDOWS" = 1 ]; then
    if [ -z "$WIN_PKG" ]; then
        step "Building the Windows package"
        ./scripts/package.sh --target windows
        WIN_PKG="$REPO_ROOT/dist/rn-win"
    fi
fi

check_pkg() {  # dir, launcher, node, installer
    for f in "$2" "runtime/bin/$3" app/src/server.ts "$4"; do
        [ -e "$1/$f" ] || die "$1 is not a complete package: $f is missing"
    done
    [ -f "$1/app/web/index.html" ] || die "$1 has no page; build without --no-web for a release"
    grep -q "signature verified" "$1/BUILD" \
        || die "$1 carries an unverified runtime; build without --no-sig for a release"
}
check_pkg "$PKG" rn node install.sh
[ "$WINDOWS" = 1 ] && check_pkg "$WIN_PKG" rn.exe node.exe install.ps1

step "Tarball"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
tar czf "$out/rn-linux-x64.tar.gz" -C "$(dirname "$PKG")" "$(basename "$PKG")"
( cd "$out" && sha256sum rn-linux-x64.tar.gz > rn-linux-x64.tar.gz.sha256 )
log "rn-linux-x64.tar.gz  $(du -h "$out/rn-linux-x64.tar.gz" | cut -f1)"
log "sha256 $(cut -d' ' -f1 "$out/rn-linux-x64.tar.gz.sha256")"

ASSETS=("$out/rn-linux-x64.tar.gz" "$out/rn-linux-x64.tar.gz.sha256")
if [ "$WINDOWS" = 1 ]; then
    step "Zip"
    # A zip rather than a tarball: Expand-Archive is in the box on Windows and
    # tar is not, on the machines this is meant to reach. install.ps1 reads the
    # .sha256 beside it, which is sha256sum's own "<hash>  <name>" format.
    command -v zip >/dev/null || die "zip is not installed; it packs the Windows asset"
    ( cd "$(dirname "$WIN_PKG")" && zip -qry "$out/rn-windows-x64.zip" "$(basename "$WIN_PKG")" )
    ( cd "$out" && sha256sum rn-windows-x64.zip > rn-windows-x64.zip.sha256 )
    log "rn-windows-x64.zip  $(du -h "$out/rn-windows-x64.zip" | cut -f1)"
    log "sha256 $(cut -d' ' -f1 "$out/rn-windows-x64.zip.sha256")"
    ASSETS+=("$out/rn-windows-x64.zip" "$out/rn-windows-x64.zip.sha256")
fi

cat > "$out/notes.md" <<NOTES
Built from \`$commit\`.

$(sed 's/^/    /' "$PKG/BUILD")

**Install** on a machine with \`gh\` signed in, no toolchains needed:

    scripts/install.sh --from-release

or by hand:

    gh release download $TAG --repo $REPO --pattern 'rn-linux-x64.tar.gz*'
    sha256sum -c rn-linux-x64.tar.gz.sha256
    tar xzf rn-linux-x64.tar.gz && rn/install.sh

It installs per-user into \`~/.local/share/rn\` with a \`rn.service\` user unit
and a menu entry; \`~/.local/share/rn/install.sh --uninstall\` removes it and
keeps \`~/.config/rn\`.

**On Windows**, no toolchain and no \`gh\` — the asset is fetched over plain
HTTPS, and the script verifies it against the checksum published beside it:

    iwr -useb https://raw.githubusercontent.com/$REPO/main/scripts/install.ps1 -OutFile install.ps1
    Unblock-File .\install.ps1
    .\install.ps1 -FromRelease

That installs into \`%LOCALAPPDATA%\Programs\rn\` with a logon scheduled task
and a Start Menu entry; \`.\install.ps1 -Uninstall\` removes it and keeps
\`%USERPROFILE%\.config\rn\`. The Windows package is cross-built on Linux and
has not yet been run on Windows — treat the first install as a review.

x64 on both platforms.
NOTES

step "Publishing $TAG to $REPO"
gh release create "$TAG" --repo "$REPO" --target "$commit" \
    --title "rn $TAG" --notes-file "$out/notes.md" "${DRAFT[@]}" "${ASSETS[@]}"
log "$(gh release view "$TAG" --repo "$REPO" --json url -q .url)"
step "Done"
log "install it anywhere with: scripts/install.sh --from-release${TAG:+ $TAG}"
