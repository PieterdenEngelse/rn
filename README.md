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
machine that runs it. The install is per-user and needs no root or
administrator.

**Windows: download
[rn-windows-x64.msi](https://github.com/PieterdenEngelse/rn/releases/latest/download/rn-windows-x64.msi)
and double-click it.** It installs into `%LOCALAPPDATA%\Programs\rn`, starts rn
at logon, adds a Start Menu entry that opens the page, and starts rn once when
it finishes. Remove it from Settings → Apps. **Releases are not code-signed
yet** — see [Smart App Control](#smart-app-control) below for what that can mean
on Windows 11.

**Which browser you download with matters.** Edge stops the MSI with
*"rn-windows-x64.msi isn't commonly downloaded. Make sure you trust
rn-windows-x64.msi before you open it."* Chrome downloads the same file without
a word. The file is not the difference: Edge applies Microsoft Defender
SmartScreen's download reputation, and an unsigned file that few people have
downloaded yet has none, so every new release starts with that warning. In
Edge the file is still yours to keep: open the downloads panel (Ctrl+J), hover
over the file, then **…** → **Keep**, and if asked, **Show more** → **Keep
anyway**. Seen on 2026-09-16 with v0.1.6: Edge warned, and Chrome finished four
downloads of it, each recorded as not dangerous. RERAG's MSI got the same Edge
warning in July. A code-signed release builds reputation that carries over from
one version to the next, which is what makes the warning go away for good.

The MSI arrived with v0.1.6. v0.1.5 and earlier have only the
[`install-rn.cmd`](https://github.com/PieterdenEngelse/rn/releases/latest/download/install-rn.cmd)
route, which asks before installing anything and names each step, and which the
[Windows](#windows-x86-64) section below still describes.

**Linux: put it in the applications menu**, since nothing a browser downloads
is executable on a click there. Copy
[`scripts/rn-install.desktop`](scripts/rn-install.desktop) in and launch
**Install rn** from the menu:

    mkdir -p ~/.local/share/applications
    curl -fsSL https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/rn-install.desktop \
      -o ~/.local/share/applications/rn-install.desktop
    chmod +x ~/.local/share/applications/rn-install.desktop
    update-desktop-database ~/.local/share/applications

It then asks what you would expect to be asked before software installs itself
— where the files go, what is downloaded, what is left alone — shows the step
it is on, and finishes by offering to open rn.

**Or one line**, which is the same install without the windows:

    curl -fsSL https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install.sh | bash -s -- --from-release

The dialogs need `zenity` or `kdialog`, which most desktops already have. On
one that has neither, the launcher opens a terminal and runs the line above in
it instead; the install is the same either way, because the windows are a face
over the same script.

The same file works as a desktop icon — copy it to `~/Desktop` and `chmod +x`
it — but that route is not reliable and the menu is. On XFCE here, a
double-click handed the file to `application/x-desktop`'s registered handler,
which is `xfce4-panel --add=launcher`: the desktop offered to add rn to the
panel instead of installing it, and no installer ever ran. Nothing in the file
causes that, and nothing in the file can prevent it — which is the argument for
leading with the menu rather than the desktop.

The scripts behind both, to read before running either — which is the better
habit with anything that installs software:

| platform | file | | |
|---|---|---|---|
| Windows | [`scripts/msi/rn.wxs`](scripts/msi/rn.wxs) | what the MSI installs and shows, and why | built by [`scripts/package-msi.ps1`](scripts/package-msi.ps1) |
| Windows | [`scripts/install-rn.cmd`](scripts/install-rn.cmd) | the double-clickable script | [download](https://github.com/PieterdenEngelse/rn/releases/latest/download/install-rn.cmd) |
| Windows | [`scripts/install-gui.ps1`](scripts/install-gui.ps1) | the dialogs around it | [download](https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install-gui.ps1) |
| Windows | [`scripts/install.ps1`](scripts/install.ps1) | what does the work | [download](https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install.ps1) |
| Linux | [`scripts/install.sh`](scripts/install.sh) | what does the work | [download](https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install.sh) |
| Linux | [`scripts/install-gui.sh`](scripts/install-gui.sh) | the dialogs around it | [download](https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install-gui.sh) |
| Linux | [`scripts/rn-install.desktop`](scripts/rn-install.desktop) | the menu entry | [download](https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/rn-install.desktop) |

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
| Windows | `rn-windows-x64.msi` | [download](https://github.com/PieterdenEngelse/rn/releases/latest/download/rn-windows-x64.msi) | [sha256](https://github.com/PieterdenEngelse/rn/releases/latest/download/rn-windows-x64.msi.sha256) |
| Windows | `rn-windows-x64.zip` | [download](https://github.com/PieterdenEngelse/rn/releases/latest/download/rn-windows-x64.zip) | [sha256](https://github.com/PieterdenEngelse/rn/releases/latest/download/rn-windows-x64.zip.sha256) |

Each package is the whole installed tree — launcher, private Node runtime,
backend and page — about 40 MB compressed. Unpack one and run the install
script inside it.

Take the newest, and the links above always do.
[v0.1.1](https://github.com/PieterdenEngelse/rn/releases/tag/v0.1.1) is the one
to avoid: its Linux launcher was built against a newer glibc than most
distributions ship and does not start on Debian 12 or Ubuntu 22.04 at all. Its
release notes say so, and v0.1.2 fixed it by linking the launcher statically
against musl. [All releases](https://github.com/PieterdenEngelse/rn/releases).

### Linux (x86-64)

**What it needs:** glibc 2.28 or newer — Debian 10, Ubuntu 20.04, RHEL 8 and
anything since — plus `curl`, `tar`, `gzip` and `sha256sum`, which `install.sh`
checks by name before it starts. The launcher links no libc at all, so that
floor is the bundled Node's rather than rn's own. Checked rather than assumed:
the release workflow installs and boots the package in clean Debian 12, Ubuntu
22.04 and Ubuntu 24.04 containers before it will publish it, and
`scripts/smoke-release.sh --tag <tag>` lets you run the same check against a
release yourself.

For the systemd user unit and the menu entry you need a systemd user session;
without one, pass `--no-service` and start `~/.local/share/rn/rn` yourself.

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

**The MSI** —
[rn-windows-x64.msi](https://github.com/PieterdenEngelse/rn/releases/latest/download/rn-windows-x64.msi)
— is the whole of it. It is a setup wizard that says what it will do before it
does anything:

1. **Welcome** — what rn is.
2. **What this installer will do** — where rn goes, the Start Menu entry, that
   it runs in the background, that your settings are never touched, and a
   checkbox for starting rn when you sign in to Windows.
3. **License** — MIT or Apache 2.0, at your option.
4. **Ready**, then **progress**.
5. **Finished** — with *Open rn in my browser now*, checked.

What it installs, and why each piece is the way it is, is written at the top of
[`scripts/msi/rn.wxs`](scripts/msi/rn.wxs):

- the package into `%LOCALAPPDATA%\Programs\rn`, with no administrator prompt;
- unless you clear the checkbox, a Run entry under your user that starts rn at
  sign-in — not the scheduled task the script route registers, because a Run
  entry is plain installer data that uninstall removes with nothing to go wrong;
- a Start Menu entry that opens <http://127.0.0.1:3010/>;
- and rn started once at the end, so the page answers straight away.

**rn opens a console window when it starts**, the one showing lines like
`"step":"listening"`. That window *is* rn: closing it stops rn. The page is in
your browser, not in that window. Uninstall from Settings → Apps stops rn first
and keeps `%USERPROFILE%\.config\rn` and your `app\.env`. **Use the MSI or the
scripts below, not both** — they install into the same directory, and the
scripts' `-Uninstall` would delete files the MSI believes it owns.

Before a release is published, the release workflow screenshots every page of
the wizard for review, and installs its MSI on a clean
Windows runner, waits for the page to answer, uninstalls it again while rn is
running and checks nothing was left behind or taken that should not have been.
That runner has no Smart App Control, so it says nothing about the next section.

**Or with the scripts**, which were the only route up to v0.1.5.
[install-rn.cmd](https://github.com/PieterdenEngelse/rn/releases/latest/download/install-rn.cmd)
is double-clickable, and nothing needs to be installed first — not even `gh`,
since the asset comes over plain HTTPS.

**What the scripts need:** Windows PowerShell 5.0 or newer, which Windows 10 and 11
ship in the box. `install.ps1` checks the version, enables TLS 1.2 before it
reaches GitHub — older Windows still defaults to TLS 1.0/1.1, which GitHub
refuses with an error naming neither — and names `Invoke-WebRequest`,
`Get-FileHash`, `Expand-Archive` and `Register-ScheduledTask` before it needs
them, rather than failing on whichever is missing.

#### Smart App Control

**Whether Smart App Control lets an unsigned rn run is not something rn
controls, and on one PC it changed within a day.** Check the setting first:
Windows Security → App & browser control → Smart App Control settings. New
Windows 11 installs often have it on, or in evaluation mode, which blocks
nothing yet but can switch itself on later.

Smart App Control lets a program run when it is signed with a certificate
Windows trusts, *or* when Microsoft's cloud reputation service already
considers that exact file safe. No release of rn is signed (see
[Code signing](#code-signing)), so everything rests on the second, which
Microsoft decides and can change without anything on the PC changing. What was
actually observed, on one Windows 11 25H2 PC with Smart App Control on the whole
time, on 2026-09-16:

- **13:24–13:26:** blocked. The downloaded `install-rn.cmd` was refused as *"a
  dangerous file extension"*, and v0.1.5's `rn.exe`, unpacked from the zip, as
  *"An Application Control policy has blocked this file"*.
- **16:10:** the v0.1.6 MSI, downloaded with Chrome, installed, started rn,
  and the page answered — no block, no warning from Smart App Control.
- **16:15:** the *same* v0.1.5 `rn.exe` that was blocked at 13:26 ran too.

So the verdict on unchanged files flipped from block to allow in under three
hours, while Windows still reported Smart App Control as On. The likeliest
reason is that Microsoft's reputation service reassessed them, but nothing on
the PC says so. Treat it as a moving target: an unsigned rn may run, may be
blocked, and a new release starts over. A trusted signature is what makes the
answer stop depending on the day.

What a block looked like, for the script route, when it happened — it stopped
rn twice, and getting past the first only reached the second:

- **The downloaded `install-rn.cmd`** is stopped with *"Smart App Control has
  blocked an app with a dangerous file extension"*. There is no Run anyway. A
  `.cmd` cannot carry a code signature at all, so no release of this file can
  pass it.
- **`rn.exe` itself** is unsigned, and is blocked however it arrives — including
  unpacked from a zip that carries no download mark: *"An Application Control
  policy has blocked this file"*. So `Unblock-File`, and the by-hand route
  below, do not help: they clear the first block and stop at this one. The
  bundled `node.exe` is signed by the OpenJS Foundation and is not affected.

Event Viewer keeps the record of a block, under Applications and Services Logs →
Microsoft → Windows → CodeIntegrity → Operational, as events 3033, 3077 and
3118. If rn is blocked there, try again later — the verdict above changed on its
own — or turn Smart App Control off on that same settings page. **Read what the page
says before you do:** Smart App Control has historically been impossible to turn
back on without resetting Windows. The fix belongs on this side: a signed MSI
with a signed `rn.exe` inside it. The MSI and the signing pipeline exist; a
certificate Windows trusts does not yet ([`docs/signing.md`](docs/signing.md)).

The same thing by hand, if you would rather watch each step:

    iwr -useb https://raw.githubusercontent.com/PieterdenEngelse/rn/main/scripts/install-gui.ps1 -OutFile install-gui.ps1
    Unblock-File .\install-gui.ps1
    .\install-gui.ps1 -FromRelease

`install-gui.ps1` is the dialogs; it runs `install.ps1`, which is the install.
Fetch that one instead to skip the windows entirely.

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

One caveat, and it is a real one: **the scripts have not been run end to end on
Windows yet.** The package is cross-built on Linux and the scripts were written
there, on a machine with no PowerShell and no Windows at all. The MSI is the
part that is installed on real Windows before every release; the scripts are
not, and the one run of them so far was stopped by Smart App Control before any
of the install could start.

What has been checked, so the caveat is not larger than it needs to be: the
launcher compiles and links for Windows as a real PE binary; every `.ps1`
parses and passes PSScriptAnalyzer in a PowerShell 7 container
(`scripts/check-ps.sh`), including the copies taken back out of the published
zip; and the guards above were exercised by extracting them from `install.ps1`
and running them. That static checking is not decoration — it caught a
null-on-the-right comparison and an output-capture scheme that could not have
worked, both before release.

What nobody has watched is the installing, and the dialogs are the least
verifiable part of it: Windows Forms cannot be loaded on Linux even in a
container, so no window in `install-gui.ps1` has ever been drawn. It falls back
to installing in the console on any host where Windows Forms will not load,
which is a path needing nothing the installer did not already need — so the
graphical half can be entirely wrong and the install should still work.

`-NoWeb -NoAutostart -NoStart` is the smallest first step, and a report of what
actually happens is worth more than anything else in this section.

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

## Code signing

**Status: no release of rn is code-signed yet.** The release workflow can sign
the Windows files — the installer `rn-windows-x64.msi`, the `rn.exe` inside it,
and the `rn.exe` inside `rn-windows-x64.zip` — with a certificate kept in the
repository's secrets, and no certificate is configured. Every release's notes
say whether its Windows files are signed.

A signature only gets rn past Smart App Control when the certificate chains to
a root Windows trusts; a self-signed one proves the pipeline and changes nothing
for a user. The bundled `node.exe` is not re-signed: it is Node's own, already
signed by the OpenJS Foundation. [`docs/signing.md`](docs/signing.md) has the
pipeline, what the certificate has to be, and how to set it up.

## Privacy

This program will not transfer any information to other networked systems
unless specifically requested by the user or the person installing or operating
it. Concretely: rn has no telemetry and no account, and it listens on loopback
only. What it sends is what its jobs are set up to send — mail you configure,
notifications to URLs you give it, feeds you list. One job runs on a schedule
without being set up first: `watch-upstreams`, daily at 04:00, fetches public
version information from nodejs.org, GitHub, crates.io and the npm registry,
sending nothing about you but the ordinary request itself. It can be set to
manual on the Jobs page.

## License

rn is licensed under either of

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT license](LICENSE-MIT)

at your option. Unless you explicitly state otherwise, any contribution
intentionally submitted for inclusion in rn by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.

The Node runtime bundled into every package is Node.js, under its own MIT
license, which ships beside it as `runtime/LICENSE`.
