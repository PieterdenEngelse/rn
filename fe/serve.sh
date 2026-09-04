#!/bin/bash
# Dev server for the rn frontend. dx defaults to :8080 — the port only comes
# from this flag, there is no Dioxus.toml key for it in dx 0.7.
#
# $PORT overrides it, so parallel worktrees can each serve their own build
# without three dx servers fighting over :1790. Unset, it stays :1790, which is
# the port the backend's CORS default and the docs both name.
#
# rn-grid.service already supplies it, one port per pane — ~/ca 1791, ~/cb
# 1792, ~/cc 1793, and the ~/rn panes 1790, which are the four RN_CORS_ORIGIN
# allows. Nothing here reproduces that mapping: a second copy of it could only
# ever disagree with the first.
cd "$(dirname "$0")"

# Which backend this build talks to. fe compiles the address in, so it has to
# be decided here rather than in the browser — and it has to agree with what
# be/s binds, which is why both read the one file.
# shellcheck source=../scripts/dev-ports.sh
. ../scripts/dev-ports.sh

# The build directory is the part the environment gets wrong — that same service
# exports one CARGO_TARGET_DIR into every pane, so without an override each
# server writes crate `fe` over the others' output. The rule moved out of here
# when be/s turned out to need the other half of it: this sets where cargo
# writes, be/s reads what that wrote, and while the two were separate be/s
# looked in a directory nothing had ever written to.
# shellcheck source=../scripts/dev-target.sh
. ../scripts/dev-target.sh

port="${PORT:-1790}"

# One server per port, decided here rather than by whoever reads the pane. dx
# binds the port exclusively, so a second one on the same port dies with
# "Address already in use" — an error that names neither the holder nor the
# fact that the holder is doing its job. Worse, dx re-emits its status panel
# after every build, so a pane that has rebuilt twice already shows two boxes;
# add a failed second server's output and the pane reads like two servers when
# it has never been anything but one. Naming the holder makes that one line.
holder=$(ss -lptnH "sport = :$port" 2>/dev/null |
    grep -o 'pid=[0-9]*' | head -1 | cut -d= -f2)
if [ -n "$holder" ]; then
    what=$(ps -o comm= -p "$holder" 2>/dev/null)
    tty=$(ps -o tty= -p "$holder" 2>/dev/null | tr -d ' ')
    since=$(ps -o lstart= -p "$holder" 2>/dev/null)
    where=$tty
    [ "$where" = "?" ] && where="no tty"
    echo "serve.sh: :$port is already served by ${what:-an exited process} (pid $holder) on ${where:-no tty}, since $since"
    # Which advice is right depends on what holds it, and the wrong half of it
    # is worse than none: telling someone to press r at a python server sends
    # them looking for a dx that is not there.
    #
    # A dx with no tty is the third case, and it was being given the second's
    # advice. "Stop it there with ctrl+c" names a pane, and a server started in
    # the background by an agent session does not have one — the line above has
    # already said "no tty" and then the next line sent the reader looking for
    # the terminal it just told them does not exist. Observed exactly that way.
    #
    # What is useful about a holder you cannot see is which tree it is serving
    # and who started it, so those are what this arm reports instead.
    if [ "$what" = "dx" ] && [ "$tty" != "?" ]; then
        echo "serve.sh: that is a dev server already. Rebuild with r in its pane, stop it there with ctrl+c."
    elif [ "$what" = "dx" ]; then
        serving=$(readlink /proc/"$holder"/cwd 2>/dev/null)
        parent=$(ps -o ppid= -p "$holder" 2>/dev/null | tr -d ' ')
        echo "serve.sh: that is a dev server with no terminal — started in the background, so there"
        echo "serve.sh: is no pane to press r or ctrl+c in. Whoever started it has to stop it."
        [ -n "$serving" ] && echo "serve.sh: it is serving ${serving/#$HOME/\~}"
        [ -n "$parent" ] && echo "serve.sh: its parent is pid $parent$( [ "$parent" = 1 ] && printf ' — orphaned, so nothing is watching it' )"
    else
        echo "serve.sh: that is not a dev server. Free the port, or serve this worktree elsewhere with PORT=."
    fi
    echo "serve.sh: nothing was started."
    exit 1
fi

# Tailwind alongside dx, because dx does not run it. A new class name in a
# component compiles fine and then does nothing, since the rule for it was
# never generated — a style that silently has no effect rather than an error,
# which is the hardest kind to attribute. dx re-bundles the stylesheet on its
# own once the file changes, so watching it is the whole of what was missing
# between "saved" and "on screen".
#
# The fifo is how it cleans itself up. Tailwind's --watch exits when its stdin
# reaches EOF — the behaviour that made a first attempt here compile once and
# stop, since a background server has no stdin. Turned around it is exactly the
# guarantee wanted: this script holds the write end, so the watcher goes away
# when the script does, including on a SIGKILL that never runs a trap. A plain
# `cmd &` plus `trap kill` leaked one watcher per restart, orphaned to PID 1,
# because the trap killed npm's shell and not the node process under it.
# `css:watch` minifies, like `css:build`. Not for the bytes — nobody serves a
# dev build to anyone — but because the file is committed, and a watcher that
# wrote it any other way left the tree permanently dirty for as long as a dev
# server ran. That is not cosmetic: rn-sync refuses to fast-forward a dirty
# tree, correctly, so a pane running ./s quietly stopped every landing from
# reaching the checkout it was serving. Observed on ~/rn, two commits behind
# main with one generated file as the only thing in its way.
# Built once, synchronously, before anything is served. The watcher only reacts
# to *changes*, so on its own it leaves whatever the last one wrote — and if that
# was a watcher running an older css:watch line, the first paint uses the wrong
# file and a `git status` reports a modified tree nobody touched. Restarting this
# script is then the whole remedy, which is the property worth having and was not
# true before. About a third of a second.
npm run css:build >/dev/null 2>&1 || echo "serve.sh: css:build failed — the page may be unstyled" >&2

fifo="$(mktemp -u)"
mkfifo "$fifo"
npm run css:watch < "$fifo" >/dev/null 2>&1 &
css_watch=$!
exec 3> "$fifo"
rm -f "$fifo"

# Only where the answer is not the documented one. This script stopped being
# `exec dx` when it took on the watcher, so its own output now lands above dx's
# startup box — and a second banner in the same terminal reads as a second
# server, which is the thing this repo has twice gone hunting for. In ~/rn the
# ports are 1790 and 3010, said in the docs and in the address bar, so the line
# buys nothing and costs that confusion. Elsewhere they are neither, and which
# backend a page is talking to is the first thing you need when it says the
# backend is unreachable.
if [ "$port" != "1790" ] || [ "$RN_API_BASE" != "http://127.0.0.1:3010" ]; then
    echo "serving $worktree on http://127.0.0.1:$port → API $RN_API_BASE"
fi

dx serve --platform web --port "$port" "$@"
