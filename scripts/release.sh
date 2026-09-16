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
#   scripts/release.sh --no-smoke      publish without the container check below
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
SMOKE=1
WINDOWS=1

log()  { printf '  %s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
die()  { printf '\nERROR: %s\n' "$*" >&2; exit 1; }
warn() { printf '  ! %s\n' "$*"; }
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
        --no-smoke)    SMOKE=0; shift ;;
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

    # install-rn.cmd rides along as an asset of its own, because the README's
    # Windows link has to be a download and a raw.githubusercontent.com URL is
    # not one: it is served as text/plain with no Content-Disposition, so a
    # click opens the script as a page of text. A release asset is served as an
    # attachment, and releases/latest/download/install-rn.cmd — the URL the
    # README uses — resolves only while every release carries it.
    #
    # Taken from the working tree rather than rewritten: .gitattributes pins it
    # to CRLF, which cmd.exe needs, and an upload keeps the bytes as they are.
    grep -q $'\r$' scripts/install-rn.cmd \
        || die "scripts/install-rn.cmd has LF endings; cmd.exe needs CRLF (see .gitattributes)"
    ASSETS+=("$REPO_ROOT/scripts/install-rn.cmd")
fi

cat > "$out/notes.md" <<NOTES
Built from \`$commit\`.

$(sed 's/^/    /' "$PKG/BUILD")

## Installing

Nothing needs to be installed first on the machine that runs rn: it carries its
own Node. The install is per-user, needs no root or administrator, and an
uninstall is a delete.

**Linux — from the applications menu.** Copy the launcher in, then click
*Install rn*:

    mkdir -p ~/.local/share/applications
    curl -fsSL https://raw.githubusercontent.com/$REPO/main/scripts/rn-install.desktop \\
      -o ~/.local/share/applications/rn-install.desktop
    chmod +x ~/.local/share/applications/rn-install.desktop
    update-desktop-database ~/.local/share/applications

It asks before installing anything, names each step while it runs, and offers
to open rn at the end. The dialogs want \`zenity\` or \`kdialog\`; with neither,
it opens a terminal and runs the same script there. A desktop icon works on
many setups but not all — on XFCE a double-click can be handed to the panel's
"Create Launcher" handler instead of being run — which is why the menu is the
instruction.

**Or one line**, the same install without the windows. No toolchain and no
\`gh\`: the repository is public, so the asset comes over plain HTTPS and is
checked against the sha256 published beside it before anything is unpacked.

    curl -fsSL https://raw.githubusercontent.com/$REPO/main/scripts/install.sh | bash -s -- --from-release $TAG

Either way it installs into \`~/.local/share/rn\` with a \`rn.service\` user unit
and a menu entry; \`~/.local/share/rn/install.sh --uninstall\` removes it and
keeps \`~/.config/rn\`.

**Windows — download
[install-rn.cmd](https://github.com/$REPO/releases/download/$TAG/install-rn.cmd)
and double-click it.** Same three windows, and nothing to install first.
Windows asks once whether you meant to run a file you downloaded; that prompt
is the mark-of-the-web check doing its job, and Run is the answer.

**With Smart App Control on, rn does not install or run**: the \`.cmd\` is
blocked as a dangerous file extension, and the unsigned \`rn.exe\` is blocked
however it arrives. The README's Smart App Control section has the detail.

Or, from a PowerShell prompt:

    iwr -useb https://raw.githubusercontent.com/$REPO/main/scripts/install-gui.ps1 -OutFile install-gui.ps1
    Unblock-File .\install-gui.ps1
    .\install-gui.ps1 -FromRelease

That installs into \`%LOCALAPPDATA%\Programs\rn\` with a logon scheduled task
and a Start Menu entry; \`.\install.ps1 -Uninstall\` removes it and keeps
\`%USERPROFILE%\.config\rn\`.

> [!IMPORTANT]
> **The Windows half has never been run on Windows.** The package is
> cross-built on Linux, and its installer dialogs use Windows Forms, which
> cannot be executed on the machine that builds these releases at all. What is
> done instead: every \`.ps1\` is parsed and linted by PowerShell 7 in a
> container (\`scripts/check-ps.sh\`), and \`install-gui.ps1\` falls back to
> installing in the console on any host where Windows Forms will not load.
> Neither is the same claim as "it works". Treat the first install as a review
> — \`-NoWeb -NoAutostart -NoStart\` is the smallest first step — and please
> report what you see.

You can check the Linux half yourself rather than taking this page's word for
it — \`scripts/smoke-release.sh --tag $TAG\` installs this release into clean
Debian and Ubuntu containers with no toolchains in them and boots it, covering
the download, the checksum, the unpack, the launcher, the bundled Node and the
page. It does not cover systemd, because a container has no user session.

x64 on both platforms.
NOTES

# The last thing before it becomes someone else's problem. v0.1.1 was published
# from a tree where every check passed, and did not start on Debian 12 or
# Ubuntu 22.04 at all: its launcher was linked against this machine's glibc, and
# no test here could see that, because every prerequisite rn has is installed
# here. The smoke test installs the package into distributions that have
# nothing, which is the only place that question can be answered.
#
# Deliberately on the built package rather than the published release: a broken
# asset that never reaches GitHub needs no announcement, no deletion and no
# superseding note. That order is the whole value — it is the step v0.1.1 did
# not have.
if [ "$SMOKE" = 1 ]; then
    step "Smoke test before publishing"
    . "$REPO_ROOT/scripts/container-runtime.sh"
    if ! rn_pick_container; then
        # Not a warning. Skipping is allowed, but it has to be asked for: a
        # release that quietly skipped its only cross-distribution check looks
        # exactly like one that passed it.
        die "the smoke test needs a container runtime: $RN_CONTAINER_HINT
       Run it elsewhere, or publish without it: --no-smoke"
    fi
    "$REPO_ROOT/scripts/smoke-release.sh" --package "$PKG" \
        || die "the package does not install and run on a clean distribution, so it was not published.
       Nothing has been uploaded and $TAG does not exist; fix it and run this again."
    log "installs and boots on every image checked (via $RN_CONTAINER)"
else
    warn "--no-smoke: this package was not installed on any distribution but this one"
fi

step "Publishing $TAG to $REPO"
gh release create "$TAG" --repo "$REPO" --target "$commit" \
    --title "rn $TAG" --notes-file "$out/notes.md" "${DRAFT[@]}" "${ASSETS[@]}"
log "$(gh release view "$TAG" --repo "$REPO" --json url -q .url)"
step "Done"
log "install it anywhere with: scripts/install.sh --from-release${TAG:+ $TAG}"
