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
# NOT CHECKED AGAINST PODMAN. There is none on this machine, so the podman path
# is argued rather than run, and this file says so rather than implying a
# coverage it does not have. The flags are ordinary enough that it should work;
# "should" is the honest word.
#
# One difference worth naming because it will not look like a runtime problem
# when it happens: on an SELinux-enforcing host — Fedora, RHEL — a plain
# `-v host:path:ro` bind mount into podman is denied, and the container sees an
# empty or inaccessible directory rather than an error that names SELinux.
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
