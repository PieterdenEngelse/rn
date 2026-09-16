#!/usr/bin/env bash
#
# Authenticode-sign a Windows file in place, from Linux.
#
#   PFX=cert.pfx PFX_PW=password scripts/sign-windows.sh dist/rn-win/rn.exe
#   PFX=cert.pfx PFX_PW=password scripts/sign-windows.sh dist/rn-windows-x64.msi
#
# What .github/workflows/release.yml runs when the WINDOWS_PFX_BASE64 secret is
# set: rn.exe first, before package-msi.sh packs it, then the MSI. The same
# pipeline RERAG's release workflow has — signtool with a .pfx, SHA-256, an RFC
# 3161 timestamp — with osslsigncode in place of signtool, because rn's Windows
# files are built on Linux and signtool only runs on Windows.
#
# The timestamp is not optional. Without one a signature stops verifying the
# day the certificate expires, and every copy already downloaded goes with it.
#
# The password comes from the environment, never from an argument, so it does
# not appear in the process list or in a log line that echoes the command.
set -euo pipefail

die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

[ $# -eq 1 ] || die "usage: PFX=cert.pfx PFX_PW=password $0 FILE"
file=$1
[ -f "$file" ] || die "$file does not exist"
[ -n "${PFX:-}" ] && [ -f "$PFX" ] || die "PFX must name the .pfx certificate"
command -v osslsigncode >/dev/null || die "osslsigncode is not installed: apt install osslsigncode"

TIMESTAMP_URL="${TIMESTAMP_URL:-http://timestamp.digicert.com}"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
chmod 700 "$work"

# -readpass takes the password from a file readable only by this user.
# -pass would put it on the command line, where any process listing shows it.
printf '%s' "${PFX_PW:-}" > "$work/pass"
osslsigncode sign \
    -pkcs12 "$PFX" -readpass "$work/pass" \
    -h sha256 \
    -n "rn" -i "https://github.com/PieterdenEngelse/rn" \
    -ts "$TIMESTAMP_URL" \
    -in "$file" -out "$work/signed" >/dev/null \
    || die "osslsigncode could not sign $file"
rm -f "$work/pass"
cat "$work/signed" > "$file"

# Checked, not trusted: the file carries a signature now. Whether Windows
# trusts it depends on the certificate, which nothing on Linux can answer —
# the Windows job reports that.
osslsigncode extract-signature -in "$file" -out "$work/sig.der" >/dev/null 2>&1 && [ -s "$work/sig.der" ] \
    || die "$file has no signature after signing"
printf '  signed %s (timestamped by %s)\n' "$file" "$TIMESTAMP_URL"
