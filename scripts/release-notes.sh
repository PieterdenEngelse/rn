#!/usr/bin/env bash
#
# Write the notes for a GitHub release to stdout.
#
#   scripts/release-notes.sh --tag v0.1.6 --commit <sha> --build dist/rn/BUILD \
#                            --signed yes|no [--repo owner/name]
#
# Called by .github/workflows/release.yml, which is what publishes releases
# now. It used to be a heredoc inside release.sh; it moved out when the build
# did, so the text lives in one file whichever way a release gets made.
#
# --signed says whether the Windows files were signed (the WINDOWS_PFX_BASE64
# secret was set). The notes say which, because with a trusted certificate the
# difference is whether Smart App Control lets rn run at all, and a reader
# deciding whether to download has to know that first.
set -euo pipefail

REPO=PieterdenEngelse/rn
TAG="" COMMIT="" BUILD="" SIGNED=""
while [ $# -gt 0 ]; do
    case $1 in
        --tag)    TAG="$2"; shift 2 ;;
        --commit) COMMIT="$2"; shift 2 ;;
        --build)  BUILD="$2"; shift 2 ;;
        --signed) SIGNED="$2"; shift 2 ;;
        --repo)   REPO="$2"; shift 2 ;;
        *)        echo "unknown option: $1" >&2; exit 2 ;;
    esac
done
[ -n "$TAG" ] && [ -n "$COMMIT" ] && [ -f "$BUILD" ] || { echo "need --tag, --commit and --build FILE" >&2; exit 2; }
case "$SIGNED" in yes|no) ;; *) echo "--signed must be yes or no" >&2; exit 2 ;; esac

DL="https://github.com/$REPO/releases/download/$TAG"

cat <<NOTES
Built from \`$COMMIT\` by the release workflow on GitHub-hosted runners.

$(sed 's/^/    /' "$BUILD")

## Installing

Nothing needs to be installed first on the machine that runs rn: it carries its
own Node. The install is per-user and needs no root or administrator.

### Windows

**Download [rn-windows-x64.msi]($DL/rn-windows-x64.msi) and double-click it.**
It installs into \`%LOCALAPPDATA%\Programs\rn\`, starts rn at logon, adds a
Start Menu entry that opens the page, and starts rn once when it finishes. It
shows Windows' own progress bar and no pages of its own. Remove it from
Settings → Apps; that keeps \`%USERPROFILE%\.config\rn\` and your
\`app\.env\`.

NOTES

if [ "$SIGNED" = yes ]; then
cat <<NOTES
The MSI and the \`rn.exe\` inside it are code-signed and timestamped. Check the
signer under the MSI's Properties → Digital Signatures: only a certificate that
Windows trusts gets rn past Smart App Control, and
[docs/signing.md](https://github.com/$REPO/blob/main/docs/signing.md) says why.

NOTES
else
cat <<NOTES
> [!WARNING]
> **This release is not code-signed.** Edge warns that the MSI "isn't commonly
> downloaded" (Chrome did not, when checked); keep it from Edge's downloads
> panel. On a PC with Smart App Control on, Windows may block the MSI or
> \`rn.exe\` with no Run anyway — or may not: it depends on Microsoft's reputation
> for these exact files, which has been seen to change within hours. The
> [README](https://github.com/$REPO#smart-app-control) has the detail.

NOTES
fi

cat <<NOTES
The MSI is installed, started, checked for a page and uninstalled again on a
clean Windows runner before this release is published. That runner has no
Smart App Control, so it cannot speak for that part.

**Without the MSI**, from a PowerShell prompt — the same package, with dialogs
that say what each step does and a logon scheduled task instead of a Run
entry:

    iwr -useb https://raw.githubusercontent.com/$REPO/main/scripts/install-gui.ps1 -OutFile install-gui.ps1
    Unblock-File .\install-gui.ps1
    .\install-gui.ps1 -FromRelease -Tag $TAG

Use one or the other, not both: they install into the same directory.

### Linux

**From the applications menu.** Copy the launcher in, then click *Install rn*:

    mkdir -p ~/.local/share/applications
    curl -fsSL https://raw.githubusercontent.com/$REPO/main/scripts/rn-install.desktop \\
      -o ~/.local/share/applications/rn-install.desktop
    chmod +x ~/.local/share/applications/rn-install.desktop
    update-desktop-database ~/.local/share/applications

It asks before installing anything, names each step while it runs, and offers
to open rn at the end. The dialogs want \`zenity\` or \`kdialog\`; with neither,
it opens a terminal and runs the same script there.

**Or one line**, the same install without the windows. The asset comes over
plain HTTPS and is checked against the sha256 published beside it before
anything is unpacked.

    curl -fsSL https://raw.githubusercontent.com/$REPO/main/scripts/install.sh | bash -s -- --from-release $TAG

Either way it installs into \`~/.local/share/rn\` with a \`rn.service\` user unit
and a menu entry; \`~/.local/share/rn/install.sh --uninstall\` removes it and
keeps \`~/.config/rn\`.

Before publishing, the Linux package was installed and booted in clean Debian 12,
Ubuntu 22.04 and Ubuntu 24.04 containers with no toolchains in them. Run
\`scripts/smoke-release.sh --tag $TAG\` to check it yourself. It does not cover
systemd, because a container has no user session.

x64 on both platforms.
NOTES
