#!/usr/bin/env bash
#
# Cut a release: tag this commit and push the tag. GitHub builds the rest.
#
#   scripts/release.sh           tag HEAD as v<version> and push it
#   scripts/release.sh --test    run the release workflow on this branch
#                                without publishing anything
#   scripts/release.sh --watch   push the tag, then follow the run
#
# The version comes from launcher/Cargo.toml, the one crate that is the app
# itself, and the workflow refuses a tag that disagrees with it.
#
# What this used to do, and why it no longer does. It built both packages here,
# smoke-tested the Linux one in containers and published from this machine.
# That order was right and is kept, in .github/workflows/release.yml: build,
# smoke test, install the MSI on a Windows runner, sign, and only then publish.
# What changed is where. SignPath Foundation signs only artifacts a
# GitHub-hosted runner built, and a Windows release that cannot be signed does
# not run on a PC with Smart App Control on (docs/signing.md). So the build
# moved, and this script is what is left for the machine to do: check that the
# commit is one anybody can rebuild, and name it.
#
# The build tools are still the same scripts — package.sh, package-msi.sh,
# smoke-release.sh — so anything the workflow does can be run here to look at
# it. Only publishing is the workflow's alone.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

log()  { printf '  %s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
die()  { printf '\nERROR: %s\n' "$*" >&2; exit 1; }
usage() { sed -n '3,10p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

TEST=0
WATCH=0
while [ $# -gt 0 ]; do
    case $1 in
        --test)    TEST=1; shift ;;
        --watch)   WATCH=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *)         echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

command -v gh >/dev/null || die "gh is not installed"
gh auth status >/dev/null 2>&1 || die "gh is not signed in: run gh auth login"

branch=$(git rev-parse --abbrev-ref HEAD)

if [ "$TEST" = 1 ]; then
    # workflow_dispatch builds and tests exactly as a tag would, and the
    # publish and sign jobs skip themselves because no tag is involved.
    git fetch -q origin "$branch" 2>/dev/null || die "$branch is not on origin; push it first"
    [ "$(git rev-parse HEAD)" = "$(git rev-parse "origin/$branch")" ] \
        || die "HEAD is not what origin/$branch holds; push first, since the runner builds origin"
    step "Running the release workflow on $branch (nothing is published)"
    gh workflow run release.yml --ref "$branch"
    sleep 3
    gh run list --workflow release.yml --branch "$branch" --limit 1
    exit 0
fi

# A release nobody can rebuild from a commit is not a release, and the runner
# builds what origin holds, not what is in this working tree.
[ -z "$(git status --porcelain)" ] || die "the tree is dirty; commit first"
[ "$branch" = main ] || die "releases are cut from main, and this is $branch"
git fetch -q --tags origin main
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] \
    || die "HEAD is not origin/main; push (or pull) first, so the tag names what the runner will build"

version=$(sed -n 's/^version = "\(.*\)"/\1/p' launcher/Cargo.toml | head -1)
[ -n "$version" ] || die "no version in launcher/Cargo.toml"
TAG="v$version"
git rev-parse -q --verify "refs/tags/$TAG" >/dev/null \
    && die "$TAG already exists; bump launcher/Cargo.toml for a new release"
gh release view "$TAG" >/dev/null 2>&1 && die "a release named $TAG already exists on GitHub"

step "Tagging $(git rev-parse --short HEAD) as $TAG"
git tag -a "$TAG" -m "rn $TAG"
git push origin "$TAG"
log "the release workflow builds, tests, signs (once configured) and publishes $TAG"

sleep 5
run=$(gh run list --workflow release.yml --event push --limit 1 --json databaseId,url -q '.[0] | "\(.databaseId) \(.url)"')
log "run: ${run#* }"
if [ "$WATCH" = 1 ]; then
    gh run watch "${run%% *}" --exit-status
fi
