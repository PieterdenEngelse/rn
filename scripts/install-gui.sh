#!/usr/bin/env bash
#
# The graphical front door to scripts/install.sh, for Linux.
#
#   scripts/install-gui.sh                 install from the package beside it
#   scripts/install-gui.sh --from-release  fetch a published release and install
#
# Every argument is handed to install.sh untouched, so this file adds a face
# and decides nothing. install.sh still does the work, still prints the same
# text, and is still the thing to read before running either.
#
# Why it exists. scripts/rn-install.desktop used to run install.sh with
# Terminal=true, which put a terminal full of shell output in front of someone
# whose first contact with rn was double-clicking an icon. That is a fine way
# to install something if you already know what a terminal is for, and a poor
# one otherwise: nothing on that screen says which parts matter, and the window
# closes on a keypress whether it succeeded or not. So the terminal is now the
# fallback rather than the default.
#
# What "graphical" means here, precisely: a confirmation before anything is
# downloaded, a progress window naming the step that is running, and a result
# window that either offers to open rn or offers the log. It is not a wizard
# and asks nothing install.sh would not have assumed — there is one install
# location and one set of defaults, and a page of choices nobody has an opinion
# about is worse than no page at all.
#
# It degrades rather than requiring anything. zenity is not part of a Linux
# install the way PowerShell is part of Windows, and rn's whole install story
# is "nothing needs to be installed first". The order is: zenity or yad, else
# kdialog, else a terminal emulator running the text installer, else xmessage
# saying what to run by hand. Only the last of those is a dead end, and it
# still names the one-line command from the README.
#
# NOT the twin of anything on Windows. install-rn.cmd is clickable but its
# install runs in a console window, because a .cmd *is* a console and giving
# Windows the same treatment means a PowerShell GUI written and unrun on Linux
# — see the note install.ps1 carries about that. The asymmetry is deliberate
# and written down here so it reads as a decision rather than an omission.
set -uo pipefail

REPO=PieterdenEngelse/rn
RAW=https://raw.githubusercontent.com/$REPO/main/scripts/install.sh
LOG="${XDG_CACHE_HOME:-$HOME/.cache}/rn-install.log"
TITLE="Install rn"
PREFIX="${XDG_DATA_HOME:-$HOME/.local/share}/rn"

# Kept deliberately, not in a mktemp directory: the result window offers to
# show it, and someone who closes that window still wants it afterwards.
mkdir -p "$(dirname "$LOG")"

# Piped — `curl ... | bash` — means there is no file on disk and no package
# beside it, exactly as install.sh handles. Same guard, same reason.
SELF="${BASH_SOURCE[0]:-}"
HERE=""
[ -n "$SELF" ] && HERE="$(cd "$(dirname "$SELF")" && pwd)"

args=("$@")

# ---------------------------------------------------------------- the installer

# Prefer the install.sh sitting next to this file: in a checkout and in a built
# package they are neighbours, and reaching over the network for a copy of a
# file that is already here would install something other than what was read.
INSTALL=""
TMPD=""
if [ -n "$HERE" ] && [ -f "$HERE/install.sh" ]; then
    INSTALL="$HERE/install.sh"
fi

# Whether install.sh will find a package beside itself decides the default
# below, so answer it the same way install.sh does rather than guessing.
have_local_package() {
    [ -n "$INSTALL" ] && [ -e "$(dirname "$INSTALL")/rn" ]
}

wants_release() {
    local a
    for a in "${args[@]+"${args[@]}"}"; do
        [ "$a" = "--from-release" ] && return 0
    done
    return 1
}

# Clicking an icon on a machine that has never seen rn is the whole point of
# this file, and there is no package in that situation. install.sh refuses and
# prints the flag to add, which is the right answer for a shell and a dead end
# for a launcher, so supply it. An explicit --from-release is left alone, and a
# real package beside us still wins.
if ! wants_release && ! have_local_package; then
    args+=(--from-release)
fi

# ------------------------------------------------------------------- dialogs

# One of zenity, yad, kdialog, or empty for none. yad is zenity's fork and
# takes the same flags for the four dialogs used here; where it differs it is
# in defaults rather than in whether the call works.
GUI=""
have_display() { [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; }
if have_display; then
    for c in zenity yad kdialog; do
        command -v "$c" >/dev/null && { GUI=$c; break; }
    done
fi

# Returns 0 for yes. kdialog's own exit codes match, so this is only a spelling
# difference.
ask() {
    case $GUI in
        zenity|yad) "$GUI" --question --title="$TITLE" --width=460 \
                        --ok-label="$2" --cancel-label="$3" --text="$1" ;;
        kdialog)    kdialog --title "$TITLE" --yes-label "$2" --no-label "$3" \
                        --yesno "$1" ;;
    esac
}

inform() {
    case $GUI in
        zenity|yad) "$GUI" --info --title="$TITLE" --width=460 --text="$1" ;;
        kdialog)    kdialog --title "$TITLE" --msgbox "$1" ;;
    esac
}

fail_dialog() {
    case $GUI in
        zenity|yad) "$GUI" --error --title="$TITLE" --width=460 --text="$1" ;;
        kdialog)    kdialog --title "$TITLE" --error "$1" ;;
    esac
}

show_log() {
    case $GUI in
        zenity|yad) "$GUI" --text-info --title="rn install log" \
                        --width=760 --height=520 --filename="$LOG" ;;
        kdialog)    kdialog --title "rn install log" --textbox "$LOG" 760 520 ;;
    esac
}

# --------------------------------------------------------- no dialogs at all

# A launcher with Terminal=false and no zenity would otherwise install in
# complete silence, so put the text installer somewhere it can be read. The
# invocation differs per emulator only in how the command is introduced.
run_in_terminal() {
    local t
    for t in x-terminal-emulator xfce4-terminal gnome-terminal konsole \
             alacritty kitty xterm; do
        command -v "$t" >/dev/null || continue
        case $t in
            gnome-terminal) exec "$t" -- bash "$INSTALL" "${args[@]}" ;;
            *)              exec "$t" -e bash "$INSTALL" "${args[@]}" ;;
        esac
    done
    return 1
}

# ------------------------------------------------------------------ fetching

# Only reached when this file was piped or copied somewhere on its own. The
# download is the same public raw URL the README gives, and failing it is the
# first thing that can go wrong on a fresh machine, so it reports properly
# rather than leaving an empty window.
fetch_installer() {
    command -v curl >/dev/null || return 2
    TMPD=$(mktemp -d) || return 2
    curl -fsSL --retry 3 -o "$TMPD/install.sh" "$RAW" || return 1
    chmod +x "$TMPD/install.sh"
    INSTALL="$TMPD/install.sh"
}

cleanup() { [ -n "$TMPD" ] && rm -rf "$TMPD"; }
trap cleanup EXIT

# --------------------------------------------------------------------- go

if [ -z "$GUI" ]; then
    # Text path. Nothing below this point needs a display, and the confirmation
    # is the terminal's own: install.sh prints what it is doing as it goes.
    [ -n "$INSTALL" ] || fetch_installer || {
        echo "could not download the installer from $RAW" >&2
        have_display && command -v xmessage >/dev/null && xmessage -center \
            "Could not download the rn installer from:
$RAW

Check the network and try again."
        exit 1
    }
    if [ -t 1 ]; then
        exec bash "$INSTALL" "${args[@]}"
    fi
    # Only worth opening a window when there is a display to open it on.
    # Without one this is a pipe, a cron job or a redirect rather than a
    # double-click, and exec'ing a terminal that cannot start leaves the caller
    # with no output and no process — observed while testing this path. The
    # same goes for xmessage below, which is an X client like any other.
    have_display && run_in_terminal
    # Nothing to run it in, and no dialog tool to say so in. xmessage is on
    # most X installs and needs no toolkit; the command it names is the one
    # from the README, so this is a detour rather than a stop.
    if have_display && command -v xmessage >/dev/null; then
        xmessage -center \
            "rn can be installed, but this machine has no zenity, kdialog
or terminal window for the installer to report in.

Open a terminal and run:

  curl -fsSL $RAW | bash -s -- --from-release"
    else
        # Either there is no display, or there is one and nothing on this
        # machine can draw a window on it. Both end here, and the remedy is
        # the same line either way, so say it rather than diagnosing which.
        echo "nothing here can show a window, so rn was not installed." >&2
        echo "run this in a terminal instead:" >&2
        echo "  curl -fsSL $RAW | bash -s -- --from-release" >&2
    fi
    exit 1
fi

# The confirmation says where the files go and what is left alone, because
# "it installed something somewhere" is the complaint an installer earns by
# not saying. The wording changes on an upgrade: replacing an install someone
# is already running is a different act from adding one.
if [ -e "$PREFIX/rn" ]; then
    verb="Upgrade rn"
    what="rn is already installed in
$PREFIX

Upgrading replaces that directory whole and restarts the service."
else
    verb="Install rn"
    what="rn will be installed for this user only, in
$PREFIX

No root, no system directories, nothing added to PATH."
fi
if wants_release; then
    where="The package comes from the latest GitHub release of $REPO,
and is checked against the sha256 published beside it before
anything is unpacked. It carries its own Node, so nothing
else has to be installed first."
else
    where="The package beside this script is installed; nothing is downloaded."
fi
ask "$what

$where

Your settings, state and credentials in ~/.config/rn are never
touched, by an install or by an uninstall." "$verb" "Cancel" || exit 0

if [ -z "$INSTALL" ]; then
    fetch_installer
    case $? in
        1) fail_dialog "Could not download the installer from:
$RAW

Check the network, then try again."; exit 1 ;;
        2) fail_dialog "This needs curl to download the installer, and curl is
not on this machine."; exit 1 ;;
    esac
fi

# Progress. install.sh announces each step with a line starting '==> ', so the
# window can name what is happening without this file keeping its own copy of
# the step list — a copy that would drift the first time install.sh grew a
# step. The bar pulses rather than filling: the total is genuinely unknown
# (a download of unknown size, then a copy), and a bar that invents a
# percentage is lying about the one thing it exists to say.
#
# install.sh runs detached from the dialog on purpose. Piping it straight into
# zenity would let a closed window kill it with SIGPIPE, halfway through
# replacing a directory. Closing the window now only stops the watching.
: > "$LOG"
bash "$INSTALL" "${args[@]}" >"$LOG" 2>&1 &
install_pid=$!

case $GUI in
    zenity|yad)
        # --no-cancel for the reason above; --auto-close because tail exits
        # with the install and the resulting EOF is the signal to close.
        tail -n +1 -f --pid="$install_pid" "$LOG" 2>/dev/null \
            | sed -u -n 's|^==> |#|p' \
            | "$GUI" --progress --pulsate --auto-close --no-cancel \
                --title="$TITLE" --width=460 --text="Starting…" >/dev/null 2>&1
        ;;
    kdialog)
        # kdialog's progress bar is a D-Bus object that has to be driven with
        # qdbus, which is a dependency for a window that would still only
        # pulse. A passive popup says the same thing and cannot fail.
        kdialog --title "$TITLE" --passivepopup \
            "Installing rn — this window closes on its own." 8 >/dev/null 2>&1
        ;;
esac
wait "$install_pid"
rc=$?

# ------------------------------------------------------------------ result

if [ "$rc" -ne 0 ]; then
    # The last non-empty line is install.sh's own die() message, which names
    # the thing that failed. The log has the rest.
    last=$(grep -v '^[[:space:]]*$' "$LOG" | tail -3)
    if ask "The install did not finish.

$last

The full log is at $LOG" "Show the log" "Close"; then
        show_log
    fi
    exit 1
fi

# install.sh prints the URL only once it has answered a health check, so its
# presence in the log is the difference between "installed" and "running".
url=$(grep -o 'http://127\.0\.0\.1:[0-9]*/' "$LOG" | tail -1)
warnings=$(sed -n 's/^  ! /• /p' "$LOG")

done_text="rn is installed in
$PREFIX

It starts at logon. The menu entry opens it."
[ -n "$warnings" ] && done_text="$done_text

Worth reading:
$warnings"

if [ -n "$url" ] && command -v xdg-open >/dev/null; then
    if ask "$done_text

It is running at $url" "Open rn" "Close"; then
        xdg-open "$url" >/dev/null 2>&1 &
    fi
else
    inform "$done_text"
fi
exit 0
