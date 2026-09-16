#!/usr/bin/env bash
#
# Wrap a Windows package in an MSI.
#
#   scripts/package-msi.sh                        dist/rn-win -> dist/rn-windows-x64.msi
#   scripts/package-msi.sh --from DIR --out FILE  other paths
#
# The package comes from `scripts/package.sh --target windows`; this adds no
# build of its own, so the MSI and the zip can never hold different trees. What
# the MSI does on the machine is written at the top of scripts/rn.wxs.
#
# Why an MSI at all, when install-rn.cmd exists: Smart App Control. It blocks a
# downloaded .cmd outright, and a .cmd cannot carry a code signature, so no
# release of that file can ever get past it. An MSI can be signed, and so can
# the rn.exe inside it, which is what the SignPath step in
# .github/workflows/release.yml is for (docs/signing.md).
#
# Needs wixl and wixl-heat, both from msitools: apt install wixl msitools.
# wixl is a WiX 3 subset that runs on Linux, which keeps the whole Windows
# release on the one build machine it already had.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FROM="$REPO/dist/rn-win"
OUT="$REPO/dist/rn-windows-x64.msi"

log()  { printf '  %s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
die()  { printf '\nERROR: %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
    case $1 in
        --from)    FROM="$(realpath -m "$2")"; shift 2 ;;
        --out)     OUT="$(realpath -m "$2")"; shift 2 ;;
        -h|--help) sed -n '3,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *)         echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

for tool in wixl wixl-heat msiinfo; do
    command -v "$tool" >/dev/null || die "$tool is not installed; it comes with msitools: apt install wixl msitools"
done
for f in rn.exe runtime/bin/node.exe app/src/server.ts BUILD; do
    [ -e "$FROM/$f" ] || die "$FROM is not a Windows package: $f is missing (scripts/package.sh --target windows)"
done

# One version, from the crate that is the app, as release.sh reads it. An MSI
# version must be three dot-separated numbers; anything else would install and
# then fail to upgrade, silently, which is worse than refusing here.
version=$(tr -d '\r' < "$REPO/launcher/Cargo.toml" | sed -n 's/^version = "\(.*\)"/\1/p' | head -1)
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] \
    || die "launcher/Cargo.toml version '$version' is not x.y.z, which an MSI version has to be"

step "Staging the package for the MSI"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
stage="$work/rn"
cp -a "$FROM" "$stage"
# The PowerShell installers stay out. They install by copying a tree and
# registering a scheduled task, and uninstall by deleting the directory; run
# against an MSI install they would remove files Windows Installer still
# believes it owns, and leave an Apps & features entry pointing at nothing.
rm -f "$stage/install.ps1" "$stage/install-gui.ps1"
# The Start Menu entry opens this. 3010 is the default port (be/src/config.ts);
# install.ps1 reads app\.env for it at install time, which an MSI built here
# cannot, so a changed BACKEND_PORT leaves this pointing at the default.
printf '[InternetShortcut]\r\nURL=http://127.0.0.1:3010/\r\n' > "$stage/rn.url"
log "files: $(find "$stage" -type f | wc -l)"

step "Harvesting the tree"
# rn.exe is declared in rn.wxs, so the launch action can name it; everything
# else is listed here. -p strips the staging prefix so the Source paths come out
# relative to $(var.SourceDir).
(cd "$work" && find rn -type f ! -path rn/rn.exe | sort \
    | wixl-heat -p rn/ --component-group AppFiles --var var.SourceDir \
                --directory-ref INSTALLDIR --win64) > "$work/files.wxs"
log "components: $(grep -c '<Component ' "$work/files.wxs")"

step "Building $(basename "$OUT") (rn $version)"
mkdir -p "$(dirname "$OUT")"
wixl -a x64 -D Version="$version" -D SourceDir="$stage" -D Win64=yes \
     -o "$OUT" "$REPO/scripts/rn.wxs" "$work/files.wxs"

# Checked, not trusted: wixl skips anything it does not understand in some
# places, so count what landed in the File table against what was staged.
staged=$(find "$stage" -type f | wc -l)
inmsi=$(msiinfo export "$OUT" File | tail -n +4 | wc -l)
[ "$staged" = "$inmsi" ] || die "the MSI holds $inmsi files but $staged were staged"
log "$(basename "$OUT")  $(du -h "$OUT" | cut -f1), $inmsi files"
