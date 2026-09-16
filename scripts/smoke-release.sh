#!/usr/bin/env bash
#
# Install rn on a machine that has nothing on it, and prove it boots.
#
#   scripts/smoke-release.sh                     the latest published release
#   scripts/smoke-release.sh --tag v0.1.2        a particular release
#   scripts/smoke-release.sh --package dist/rn   a package built here, before publishing
#   scripts/smoke-release.sh --image debian:11   one image instead of the default three
#
# Why this exists. Every prerequisite rn has is installed on the machine that
# builds it, so that machine cannot answer the only question that matters about
# a release: does it work for someone else? Asked properly for the first time on
# 2026-09-16, the answer was no. v0.1.1's launcher was built with a plain
# `cargo build` against this machine's glibc, recorded GLIBC_2.39, and would not
# start on Debian 12, Ubuntu 22.04, Mint 21 or RHEL 9 — it failed before main(),
# so rn's own error reporting could not reach it, and nothing in the tree said
# so. The bundled Node was fine back to Debian 10; only the launcher was the
# limit. `package.sh` now refuses to ship a Linux launcher carrying a GLIBC_
# symbol, and this script is the other half: the guard proves a property of the
# binary, this proves the whole chain.
#
# What it actually covers, end to end, in a container with no rustc, node, npm,
# gh or git: fetching install.sh over HTTPS, downloading the release asset,
# verifying it against its published sha256, unpacking, installing, starting the
# launcher, the launcher starting the bundled Node, all three listeners coming
# up, /api/health answering, and the page returning 200.
#
# What it does NOT cover, and should not be read as covering:
#
#   - systemd. A container has no user session, so the install runs
#     --no-service. The unit, the menu entry and the port-in-use check are not
#     exercised here.
#   - the graphical installer. No X in a container, so install-gui.sh's dialogs
#     are untested by this; a desktop VM is the only honest way to check those.
#   - anything but x86_64 glibc Linux.
#
# The health probe uses the bundled node rather than curl, so the probe needs
# nothing installed and cannot itself be the reason a run fails. curl is still
# installed for --from-release, because install.sh downloads with it.
set -euo pipefail

REPO=PieterdenEngelse/rn
IMAGES=(debian:12 ubuntu:22.04 ubuntu:24.04)
TAG=""
PKG=""
PREFIX=/root/.local/share/rn

die()  { printf '\nERROR: %s\n' "$*" >&2; exit 1; }
step() { printf '\n==> %s\n' "$*"; }
log()  { printf '  %s\n' "$*"; }

images_given=0
while [ $# -gt 0 ]; do
    case $1 in
        --tag)     TAG="$2"; shift 2 ;;
        --package) PKG="$(realpath "$2")"; shift 2 ;;
        --image)   [ "$images_given" = 1 ] || IMAGES=(); images_given=1
                   IMAGES+=("$2"); shift 2 ;;
        --repo)    REPO="$2"; shift 2 ;;
        -h|--help) sed -n '3,9p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *)         die "unknown option: $1" ;;
    esac
done

[ -n "$PKG" ] && [ -n "$TAG" ] && die "--package and --tag are different sources; pick one"
[ -n "$PKG" ] && [ ! -e "$PKG/rn" ] && die "$PKG holds no rn launcher — build one with scripts/package.sh"

command -v docker >/dev/null || die "this needs docker, which is not on PATH"
docker info >/dev/null 2>&1 || die "docker is installed but not usable as this user"

# Written out rather than passed as -c, so the quoting inside it is its own and
# the failure messages survive. Mounted read-only, like the package.
runner=$(mktemp); trap 'rm -f "$runner"' EXIT
cat > "$runner" <<'INNER'
set -u
RN=/root/.local/share/rn
say() { printf '  %s\n' "$*"; }

. /etc/os-release
say "distro:  $PRETTY_NAME"
say "glibc:   $(ldd --version 2>/dev/null | head -1 | grep -o '[0-9.]*$')"
missing=""
for t in rustc node npm gh git cargo; do command -v "$t" >/dev/null || missing="$missing $t"; done
say "absent: $missing"

if [ -d /pkg ]; then
    cp -a /pkg /work
    say "source:  a package built on the host"
else
    # install.sh downloads with curl, so a minimal image needs it. This is the
    # only apt call, and a distro whose repositories have been archived (an EOL
    # one) fails here rather than at anything to do with rn — so say which.
    if ! command -v curl >/dev/null; then
        apt-get -qq update >/dev/null 2>&1 && apt-get -qq install -y curl ca-certificates >/dev/null 2>&1 || {
            echo "SKIP: could not install curl here (archived apt repositories on an EOL release?)"; exit 3; }
    fi
    say "source:  $RELEASE_DESC"
fi

echo
if [ -d /work ]; then
    /work/install.sh --no-service --prefix "$RN" 2>&1 | grep -E '^==>|node:|build:|page:|installed,' || true
else
    curl -fsSL "https://raw.githubusercontent.com/$REPO/main/scripts/install.sh" \
      | bash -s -- --from-release ${TAG:+"$TAG"} --no-service --prefix "$RN" 2>&1 \
      | grep -E '^==>|sha256|node:|build:|page:|installed,' || true
fi
[ -x "$RN/rn" ] || { echo "FAIL: nothing was installed at $RN"; exit 1; }

echo
cd "$RN/app"
"$RN/rn" > /tmp/boot.log 2>&1 &
lp=$!

# The bundled node is the probe, so nothing has to be installed for it and a
# missing curl can never be mistaken for a broken backend.
probe() {
    "$RN/runtime/bin/node" -e '
      // argv[1], not argv[2]: with `node -e` there is no script filename in
      // between, so the usual [,,a,b] skips one argument too many and fetches
      // the literal string "body" as a URL.
      const [,url,mode]=process.argv;
      fetch(url).then(r=>r.text().then(t=>{
        process.stdout.write(mode==="body" ? r.status+" "+t.slice(0,110) : String(r.status));
        process.exit(r.ok?0:1);
      })).catch(()=>process.exit(1));
    ' "$1" "${2:-status}" 2>/tmp/probe.err
}

ok=0
for _ in $(seq 1 60); do
    if health=$(probe http://127.0.0.1:3010/api/health body); then ok=1; break; fi
    sleep 0.5
done

if [ "$ok" = 1 ]; then
    say "health:  $health"
    page=$(probe http://127.0.0.1:3010/ status) && say "page:    HTTP $page" || say "page:    not served (a --no-web package?)"
    say "boot:    $(grep -o '"step":"[a-z-]*listening"' /tmp/boot.log | tr '\n' ' ')"
fi
kill $lp 2>/dev/null
wait $lp 2>/dev/null

if [ "$ok" != 1 ]; then
    echo "FAIL: the backend never answered /api/health"
    # Both halves, because "no answer" has two very different causes and the
    # boot log alone cannot tell them apart: the backend never came up, or the
    # probe itself is broken. The second one wasted a run while being written.
    echo "-- last probe error --"; tail -3 /tmp/probe.err 2>/dev/null
    echo "-- backend log --";      tail -8 /tmp/boot.log
    exit 1
fi
echo "PASS"
INNER

if [ -n "$PKG" ]; then
    source_desc="the package at $PKG"
else
    source_desc="${TAG:-the latest release} from $REPO"
fi
step "Smoke test: installing $source_desc on ${#IMAGES[@]} image(s)"
log "no rustc, node, npm, gh or git in any of them"

declare -a results=()
failed=0
passed=0
for img in "${IMAGES[@]}"; do
    step "$img"
    mounts=(-v "$runner:/run.sh:ro")
    [ -n "$PKG" ] && mounts+=(-v "$PKG:/pkg:ro")
    set +e
    docker run --rm --pull=missing "${mounts[@]}" \
        -e "REPO=$REPO" -e "TAG=$TAG" -e "RELEASE_DESC=$source_desc" \
        "$img" bash /run.sh
    rc=$?
    set -e
    case $rc in
        0) results+=("PASS  $img"); passed=$((passed + 1)) ;;
        3) results+=("SKIP  $img  (could not install curl)") ;;
        *) results+=("FAIL  $img"); failed=1 ;;
    esac
done

step "Summary"
printf '  %s\n' "${results[@]}"
if [ "$failed" = 1 ]; then
    die "at least one image could not install and run $source_desc"
fi
# A run where everything skipped verified nothing, and saying "it works on
# every image checked" would be true and useless — the set checked was empty.
# Worth distinguishing, because a skip is quiet and a green summary is not.
if [ "$passed" = 0 ]; then
    die "nothing was verified: every image skipped, so $source_desc is untested"
fi
log "$source_desc installs and boots on every image checked ($passed)"
