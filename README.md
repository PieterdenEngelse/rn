# rn

A personal automation app you install on your own machine: it watches things,
reacts to them, tells you about it, and gives you a page to configure and
watch it from.

Node drives the automation. A small Rust launcher supervises it and carries a
**private Node runtime**, so rn does not use — or need — whatever Node is on
the machine. The page is a Dioxus/wasm frontend served by the backend itself,
on one origin, with no dev server involved.

It is a personal project, not a product. There is no hosted version, no
account, and no telemetry. It runs on loopback on your machine unless you go
out of your way.

## What it does today

Ten jobs ship. The runner has three front doors — by hand from the page, the
scheduler, or an inbound webhook — and a job says for itself which of them
apply to it.

| job | what it does |
|---|---|
| `watch-upstreams` | the versions rn pins, and when one of them moves |
| `watch-feeds` | feeds, reporting only what you have not been told about |
| `watch-deliveries` | GitHub's own record of webhook deliveries, and which failed |
| `read-mail` / `send-mail` | IMAP in, SMTP out |
| `notify` / `notify-all` | a change, sent to a URL — or to every notifier at once |
| `desktop-notify` | the same, drawn on the desktop of the machine rn runs on |
| `prune-profiles` | deletes the V8 profiling artifacts that accumulate silently |
| `webhook-echo` | what an inbound delivery actually contained, for setting one up |

Around them: a scheduler, a webhook listener on its own port, click tracking
for mail you send, a credentials store that is written but never read back,
and monitor pages for the runtime, jobs, connections, mail and links.

Jobs start in **dry-run** — `DRY_RUN=true` is the shipped default, and
anything other than the exact string `false` keeps it on. A job that has never
run is not a job you trust yet.

## Installing

rn carries its own Node, so **nothing needs to be installed first** on the
machine that runs it. The install is per-user, needs no root, and an uninstall
is a delete.

**Windows: download
[install-rn.cmd](https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install-rn.cmd)
and double-click it.** It fetches the installer and the published package,
checks the package against its published checksum, and installs. Windows asks
once whether you meant to run a file you downloaded; that prompt is the
mark-of-the-web check doing its job, and Run is the answer.

**Linux: one line**, because nothing a browser downloads is executable on a
click there either:

    curl -fsSL https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install.sh | bash -s -- --from-release

Or a clickable icon, if you would rather have one:
[`scripts/rn-install.desktop`](scripts/rn-install.desktop) is a launcher that
runs exactly that line in a terminal and waits before closing, so you can read
what it did. Copy it to `~/.local/share/applications/` for a menu entry, or to
`~/Desktop` and `chmod +x` it for an icon on the desktop.

The scripts behind both, to read before running either — which is the better
habit with anything that installs software:

| platform | file | | |
|---|---|---|---|
| Windows | [`scripts/install-rn.cmd`](scripts/install-rn.cmd) | the double-clickable one | [download](https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install-rn.cmd) |
| Windows | [`scripts/install.ps1`](scripts/install.ps1) | what it runs | [download](https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install.ps1) |
| Linux | [`scripts/install.sh`](scripts/install.sh) | what the line above runs | [download](https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install.sh) |
| Linux | [`scripts/rn-install.desktop`](scripts/rn-install.desktop) | a clickable launcher for it | [download](https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/rn-install.desktop) |

Either one is enough on its own: both download a published package and install
it, needing no toolchain, no clone and nothing else on the machine, and both
verify the download against the checksum published beside it before unpacking
anything. `install.ps1` additionally knows how to build rn from a checkout,
for when you have one — [Download ZIP](https://github.com/PieterdenEngelse/rn/archive/refs/heads/main.zip)
or `git clone https://github.com/PieterdenEngelse/rn.git`.

The packages they fetch, if you would rather take one by hand. These links
always resolve to the newest release, so they do not go stale here:

| platform | package | | |
|---|---|---|---|
| Linux | `rn-linux-x64.tar.gz` | [download](https://github.com/PieterdenEngelse/rn/releases/latest/download/rn-linux-x64.tar.gz) | [sha256](https://github.com/PieterdenEngelse/rn/releases/latest/download/rn-linux-x64.tar.gz.sha256) |
| Windows | `rn-windows-x64.zip` | [download](https://github.com/PieterdenEngelse/rn/releases/latest/download/rn-windows-x64.zip) | [sha256](https://github.com/PieterdenEngelse/rn/releases/latest/download/rn-windows-x64.zip.sha256) |

Each package is the whole installed tree — launcher, private Node runtime,
backend and page — about 40 MB compressed. Unpack one and run the install
script inside it. [All releases](https://github.com/PieterdenEngelse/rn/releases):
[v0.1.1](https://github.com/PieterdenEngelse/rn/releases/tag/v0.1.1) is the
first to carry a Windows package, cross-built on Linux.

### Linux (x86-64)

From a published release, which needs no toolchain and no account. `install.sh`
uses `gh` when it is installed and signed in, and plain `curl` otherwise —
release assets on a public repository need no token:

    curl -fsSL https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install.sh | bash -s -- --from-release

Or saved first, if you would rather read it before it runs:

    curl -fsSLO https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install.sh
    chmod +x install.sh
    ./install.sh --from-release

It verifies the asset against the sha256 published beside it before unpacking
anything. From a clone, it is the same script:

    scripts/install.sh --from-release

Or build it yourself, which needs Rust, Node and `dx` (dioxus-cli):

    scripts/package.sh          # → dist/rn   (--no-web skips the wasm page build)
    dist/rn/install.sh          # → ~/.local/share/rn, a systemd user unit, a menu entry

Either way you end up with `~/.local/share/rn`, an `rn.service` that starts at
login, and a menu entry that opens the page.

    ~/.local/share/rn/install.sh --uninstall

### Windows (x86-64)

Double-clicking
[install-rn.cmd](https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install-rn.cmd)
is the whole of it, and nothing needs to be installed first — not even `gh`,
since the asset comes over plain HTTPS.

The same thing by hand, if you would rather watch each step:

    iwr -useb https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install.ps1 -OutFile install.ps1
    Unblock-File .\install.ps1
    .\install.ps1 -FromRelease

`Unblock-File` is not optional there: a file downloaded from the internet is
marked as such, and PowerShell refuses to run what is marked. The `.cmd` avoids
it by fetching the script itself rather than having you download it. The same
mark applies to every `.ps1` in the repository if you take it as a ZIP.

Or build it from a checkout, which needs Rust, Node and `dx` on the Windows
machine itself:

    .\scripts\install.ps1              # -NoWeb skips the wasm page build
    .\scripts\install.ps1 -Uninstall

Either way you get `%LOCALAPPDATA%\Programs\rn`, a scheduled task that starts
it at logon, and a Start Menu entry.

One caveat, and it is a real one: **none of this has been run on Windows yet.**
The package is cross-built on Linux and the scripts were written there. What
has been checked is everything up to the install itself — the launcher compiles
and links for Windows, the scripts parse and lint clean, and the whole
`-FromRelease` path was run against the live release in PowerShell: download,
checksum, unpack, every expected file in place. What nobody has watched is the
installing. `-NoWeb -NoAutostart -NoStart` is the smallest first step, and the
Linux install remains the tested one.

## After installing

Open <http://127.0.0.1:3010/> — the page and the API share that port. The
webhook listener and the click tracker sit on their own ports beside it, so a
tunnel pointed at one cannot reach the API's mutating routes.

Two directories matter, and only one of them is yours:

- **`~/.config/rn`** (`%USERPROFILE%\.config\rn` on Windows) — settings,
  state and credentials. Installs and uninstalls never touch it.
- **the install directory** — replaced wholesale on every upgrade. Nothing you
  care about may live there. The one file carried across is `app/.env`.

**The API has no authentication.** It binds loopback, and rn refuses to start
on a routable address without a second explicit opt-in, because that bind
address is the whole of the security position. Read `docs/network.md` before
changing it — a tunnel is almost always the better answer, and
`docs/tunnel.md` covers the one that is actually in use here.

Settings that exist and what each costs: `be/.env.example` is the reference,
and a test fails if a setting is missing from it.

## Developing

    ./scripts/setup.sh          # Windows: .\scripts\setup.ps1
    ./scripts/check.sh          # Windows: .\scripts\check.ps1 — run before committing

`check.sh` covers the Rust workspace and the Node backend together, because for
a while only one of them was being run and nothing said so. Commands, ports and
the scratch-runtime recipe are in `docs/dev.md`.

## Layout

    be/         the Node backend — the automation surface
    fe/         the Dioxus frontend, compiled to wasm
    shared/     wire types both ends agree on, defined once
    launcher/   the Rust launcher: spawns and supervises Node
    scripts/    setup, checks, packaging, install
    docs/       why things are the way they are

`CLAUDE.md` holds the rules and the reasoning; `docs/` is indexed at the end of
it. Worth knowing before reading the code: every value crossing the Node/Rust
boundary is defined once in `shared/`, and the launcher spawns Node with an
environment built from nothing rather than inherited — `docs/packaging.md`
says why both of those are load-bearing rather than fastidious.
