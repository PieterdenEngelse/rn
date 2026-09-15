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

| | |
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

There is one install script per platform:

| | | |
|---|---|---|
| Linux | [`scripts/install.sh`](scripts/install.sh) | [download](https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install.sh) |
| Windows | [`scripts/install.ps1`](scripts/install.ps1) | [download](https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install.ps1) |

**Only the Linux one is useful on its own.** `install.sh` can fetch a published
release and install it, so the single file is enough. `install.ps1` builds rn
from the checkout it sits in — there is no Windows release for it to download —
so downloading that one file alone gets you a script with nothing to build.
For Windows, take the whole repository:
[Download ZIP](https://github.com/PieterdenEngelse/rn/archive/refs/heads/main.zip),
or `git clone https://github.com/PieterdenEngelse/rn.git`.

### Linux (x86-64)

From a published release, which needs no toolchain at all — only `gh`, signed
in, because that is what `install.sh` downloads the release asset with:

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

There is no Windows package yet, so one script builds and installs in a single
pass. That means the machine needs Rust, Node and — unless you pass `-NoWeb` —
`dx`:

    .\scripts\install.ps1
    .\scripts\install.ps1 -Uninstall

If you downloaded the repository as a ZIP, Windows marks everything in it as
coming from the internet and PowerShell will refuse to run it. Clear that
first, from the repository root:

    Unblock-File .\scripts\*.ps1

You get `%LOCALAPPDATA%\Programs\rn`, a scheduled task that starts it at logon,
and a Start Menu entry. **This path has not been run on Windows yet**; the
script's own header says so and suggests `-NoWeb -NoAutostart -NoStart` as a
first step. The Linux install is the tested one.

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
