#!/usr/bin/env bash
#
# Publish a packaged rn as a GitHub release, so a machine with no Rust, Node
# or dx can install it with one command:
#
#   scripts/release.sh                 build this checkout and publish v<version>
#   scripts/release.sh --tag v0.1.1    a tag of your choosing
#   scripts/release.sh --from dist/rn  publish a package already built
#   scripts/release.sh --draft         publish as a draft, to look at first
#
# Then, on any machine with gh signed in:
#
#   scripts/install.sh --from-release
#
# The version comes from launcher/Cargo.toml, the one crate that is the app
# itself. The tree must be clean: a release nobody can rebuild from a commit
# is not a release, and the BUILD file inside the package records the commit
# it came from.
#
# Linux x64 only, like package.sh (docs/packaging.md §9).
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
REPO=PieterdenEngelse/rn
PKG=""
TAG=""
DRAFT=()
ALLOW_DIRTY=0

log()  { printf '  %s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
die()  { printf '\nERROR: %s\n' "$*" >&2; exit 1; }
usage() { sed -n '3,13p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
    case $1 in
        --tag)         TAG="$2"; shift 2 ;;
        --from)        PKG="$(realpath -m "$2")"; shift 2 ;;
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
for f in rn runtime/bin/node app/src/server.ts install.sh; do
    [ -e "$PKG/$f" ] || die "$PKG is not a complete package: $f is missing"
done
[ -f "$PKG/app/web/index.html" ] || die "$PKG has no page; build without --no-web for a release"
grep -q "signature verified" "$PKG/BUILD" \
    || die "$PKG carries an unverified runtime; build without --no-sig for a release"

step "Tarball"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
tar czf "$out/rn-linux-x64.tar.gz" -C "$(dirname "$PKG")" "$(basename "$PKG")"
( cd "$out" && sha256sum rn-linux-x64.tar.gz > rn-linux-x64.tar.gz.sha256 )
log "rn-linux-x64.tar.gz  $(du -h "$out/rn-linux-x64.tar.gz" | cut -f1)"
log "sha256 $(cut -d' ' -f1 "$out/rn-linux-x64.tar.gz.sha256")"

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
keeps \`~/.config/rn\`. Linux x64 only.
NOTES

step "Publishing $TAG to $REPO"
gh release create "$TAG" --repo "$REPO" --target "$commit" \
    --title "rn $TAG" --notes-file "$out/notes.md" "${DRAFT[@]}" \
    "$out/rn-linux-x64.tar.gz" "$out/rn-linux-x64.tar.gz.sha256"
log "$(gh release view "$TAG" --repo "$REPO" --json url -q .url)"
step "Done"
log "install it anywhere with: scripts/install.sh --from-release${TAG:+ $TAG}"
