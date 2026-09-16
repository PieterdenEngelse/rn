#!/usr/bin/env bash
#
# Which container runtime to use, decided once.
#
# Sourced by scripts/smoke-release.sh, scripts/check-ps.sh and
# scripts/release.sh, the three places that need a userspace that is not this
# machine's. It lives here for the same reason scripts/dev-ports.sh does: three
# copies of a decision can only ever disagree with each other.
#
#     . "$(dirname "$0")/container-runtime.sh"
#     rn_pick_container || die "$RN_CONTAINER_HINT"
#     "$RN_CONTAINER" run --rm ...
#
# Docker first, then podman, and RN_CONTAINER overrides both:
#
#     RN_CONTAINER=podman scripts/smoke-release.sh
#
# Docker leads only because it is what these scripts were written and checked
# against, not because anything here needs it. Every call they make is `info`
# and `run --rm --pull=missing -v src:dst:ro -e VAR image cmd`, which podman
# takes with the same spelling — rootless, which is the better default of the
# two. If podman is the one you have, nothing here should notice.
#
# Both are run, not argued. Checked with podman 5.7.0 rootless: check-ps.sh
# parses and lints clean through it, and smoke-release.sh installs and boots the
# package on all three default images, as does the release gate. The bind
# mounts, the read-only flags and the -e variables behave identically; rootless
# ownership mapping did not get in the way of anything the install does.
#
# It did find one thing, which is why running it beat reasoning about it: image
# names are now fully qualified in smoke-release.sh. Docker resolves a bare
# `debian:12` against Docker Hub; podman does not, and only resolved it here
# because Ubuntu ships an alias for it in registries.conf.d/shortnames.conf.
# On a podman host without that, a short name errors or prompts, and says so in
# terms of a registry rather than of this project.
#
# Still untested, and named rather than implied: SELinux. This machine does not
# enforce it, so the one difference below is documented from podman's behaviour
# rather than from having hit it. On Fedora or RHEL a plain `-v host:path:ro`
# bind mount into podman is denied, and the container sees an empty or
# inaccessible directory rather than an error that names SELinux.
# RN_CONTAINER_RUN_OPTS is the way out without this file guessing:
#
#     RN_CONTAINER_RUN_OPTS="--security-opt label=disable" scripts/check-ps.sh
#
# That is preferred over adding `:z` to the mounts, which would relabel files in
# your working tree as a side effect of running a test.

# Sets RN_CONTAINER to a usable runtime and returns 0, or returns 1 and leaves
# RN_CONTAINER_HINT saying what was tried.
rn_pick_container() {
    local candidates="${RN_CONTAINER:-docker podman}"
    local c found_but_unusable=""
    for c in $candidates; do
        command -v "$c" >/dev/null 2>&1 || continue
        if "$c" info >/dev/null 2>&1; then
            RN_CONTAINER="$c"
            export RN_CONTAINER
            return 0
        fi
        found_but_unusable="$found_but_unusable $c"
    done
    if [ -n "$found_but_unusable" ]; then
        RN_CONTAINER_HINT="installed but not usable as this user:$found_but_unusable
       (docker usually means the daemon is down or you are not in the docker group)"
    else
        RN_CONTAINER_HINT="no container runtime found. Tried: $candidates
       Install docker or podman, or set RN_CONTAINER to the one you have."
    fi
    RN_CONTAINER=""
    return 1
}

# The options every `run` should carry. Empty unless the caller sets
# RN_CONTAINER_RUN_OPTS; word-split on purpose, so a caller can pass several.
rn_container_run_opts() {
    printf '%s' "${RN_CONTAINER_RUN_OPTS:-}"
}
