# Rebuilding this machine with chezmoi

Plan, written 2026-09-11. **Phases 1 to 4 exist:** the private source repo
`PieterdenEngelse/dotfiles`, checked out at `~/.local/share/chezmoi`, holds
`capture.sh`, the curated package lists, the dotfiles, the age-encrypted
secrets and the bootstrap scripts. chezmoi v2.72.1 is in `~/.local/bin`.
The restore test (Phase 5) has run in a container, which leaves snaps, user
services and the desktop login for a VM run that is still open. It's about the
machine rn runs on, not about rn itself, but it lives here because
rebuilding the machine is mostly rebuilding what rn needs.

Decided so far:

- The source repo is **private GitHub**.
- The current ssh key is carried over, not regenerated (see Secrets).
- The `rn-*` helpers live in rn's own `scripts/`, done 2026-09-11.
  `~/.local/bin` holds symlinks to them, so chezmoi manages the links and
  never the scripts (see Phase 2).
- **Both desktops stay:** `ubuntu-desktop` (GNOME) alongside `xfce4`. So the
  dconf step stays, and so does the snap base GNOME brings. XFCE is the
  session actually used, through **LightDM**, and the rebuild has to
  reproduce that deliberately (see `20-apt` in Phase 4).

**The goal:** a fresh Ubuntu install gets from a blank desktop to this one with
two commands: one to authenticate, one to apply.

    sudo apt-get install -y gh && gh auth login     # HTTPS; let it set up git
    sh -c "$(curl -fsLS get.chezmoi.io)" -- init --apply PieterdenEngelse/dotfiles

It can't be one command. The repo is private, and nothing on a blank machine
can authenticate to GitHub until `gh` is signed in. chezmoi clones over
HTTPS, and `gh auth login` sets up the git credential helper that answers for
it. That is also how rn is pushed: this machine's ssh key isn't registered
on GitHub (checked 2026-09-11: `Permission denied (publickey)` from a key
with no passphrase), so an ssh clone fails here as well. The `gh auth login`
has to happen before chezmoi runs, even though the `.gitconfig` that
chezmoi writes carries the same helper lines.

After that, a handful of steps stay manual and say so, because nobody can do
them for you: signing in to Tailscale, rclone and the browsers.

## What there is to capture (surveyed 2026-09-11)

| Area | Current state | Where it lives |
|---|---|---|
| OS | Ubuntu 26.04 LTS, Huawei BOHB-WAX9 laptop | — |
| Desktop | XFCE on X11, with `ubuntu-desktop` also installed | `~/.config/xfce4/xfconf/xfce-perchannel-xml/*.xml` (18 channels) |
| GNOME leftovers | ibus, tiling-assistant | `dconf` database |
| Window rules | devilspie2 | `~/.config/devilspie2/screen_assign.lua` |
| Autostart | devilspie2, remmina applet, firefox, monitor terminals, click-show-desktop, clipman, notes | `~/.config/autostart/*.desktop` |
| apt | 88 packages marked manual, most of them installer noise | `apt-mark showmanual` |
| Third-party apt repos | docker, tailscale, vscode | `/etc/apt/sources.list.d/`, keyrings |
| snap | firefox, thunderbird, chromium, htop (everything else is base) | `snap list` |
| VS Code | 9 extensions | `code --list-extensions`, `~/.config/Code/User/` |
| Toolchains | rustup + wasm32 plus `dioxus-cli`, nvm (24.19, 24.20), deno, Claude Code. `~/.bun` and `~/.dotnet` are leftovers, not installs: no `bun` binary, and only a cache under `.dotnet` | upstream installers |
| User binaries | `ag`, `rn-*`, `audio-*`, `rtk`, `claude`, llama.cpp, xfce helpers | `~/.local/bin` |
| User units | `ag`, `audio-guard`, `falkordb`, `n8n-alerts`, `rclone-gdrive/onedrive`, `rn-backend`, `rn-grid` | `~/.config/systemd/user/` |
| System units | `audio-amp-guard` (enabled). `ollama` is disabled, and its installer writes the unit itself, so it isn't captured | `/etc/systemd/system/` |
| Hardware quirks | `es8336-quirk.conf` (`quirk=0x1b0`). `iwlwifi.conf` and `alsa-base.conf` belong to `kmod` and `alsa-base`, unmodified (`dpkg -V`), so a fresh install brings them | `/etc/modprobe.d/` |
| Shell | `.bashrc`, `.profile`, `.tmux.conf`, `.gitconfig` | `~` |
| Containers | n8n (stopped deliberately since 2026-09-10) | docker |
| **Secrets** | ssh key, rclone OAuth tokens, `~/.config/rn/credentials` | see "Secrets" below |

Things to leave out on purpose: the `*.bak*` files next to everything, the
screenshots scattered around `~`, `~/.cache`, `~/.npm`, the nvm version
directories, the snap-generated udev rules, and `~/.config/rn/job-*.json`,
which is runtime state rather than configuration.

## Layout of the chezmoi source repo

A **private** GitHub repo, `PieterdenEngelse/dotfiles`, checked out at
`~/.local/share/chezmoi`:

    .chezmoi.toml.tmpl              # encryption, age identity + recipient, umask
    secrets/                        # age-encrypted values that templates read
    .chezmoiignore                  # *.bak*, and hardware files on other machines
    .chezmoiexternal.toml           # nvm, tpm, anything fetched rather than stored

    packages/
        apt.txt                     # curated, one per line, comments allowed
        apt-repos/                  # docker.list, tailscale.list, vscode.sources + keys
        snap.txt
        vscode-extensions.txt
        cargo.txt                   # dioxus-cli
    desktop/
        dconf.ini                   # filtered `dconf dump`
    system/                         # mirrors /etc paths; installed with sudo
        all/etc/lightdm/lightdm.conf.d/50-rn.conf
        bohb-wax9/etc/modprobe.d/es8336-quirk.conf
        bohb-wax9/etc/systemd/system/audio-amp-guard.service
    lib/common.sh                   # helpers every run_ script sources

    dot_bashrc
    dot_profile
    dot_tmux.conf
    dot_gitconfig.tmpl
    dot_config/xfce4/xfconf/xfce-perchannel-xml/*.xml
    dot_config/autostart/*.desktop
    dot_config/devilspie2/screen_assign.lua
    dot_config/systemd/user/*.service …
    dot_config/Code/User/settings.json, keybindings.json
    private_dot_config/rclone/encrypted_rclone.conf.age
    private_dot_ssh/encrypted_private_id_ed25519.age
    dot_local/bin/executable_*      # ag, audio-*, xfce helpers
    dot_local/bin/symlink_rn-*.tmpl # each one line: ~/rn/scripts/rn-…

    run_once_before_00-prereqs.sh.tmpl
    run_onchange_before_10-apt-repos.sh.tmpl
    run_onchange_after_20-apt.sh.tmpl
    run_onchange_after_21-snap.sh.tmpl
    run_once_after_30-toolchains.sh.tmpl
    run_onchange_after_31-cargo.sh.tmpl
    run_onchange_after_32-vscode.sh.tmpl
    run_onchange_after_33-node-keys.sh.tmpl
    run_onchange_after_40-system-files.sh.tmpl
    run_onchange_after_50-dconf.sh.tmpl
    run_onchange_after_51-xfconf.sh.tmpl
    run_once_after_60-user-units.sh.tmpl
    run_once_after_70-projects.sh.tmpl
    run_once_after_99-checklist.sh.tmpl

Every script is a template, if only to learn where the source directory is
so it can source `lib/common.sh` and read the lists.

### Why this layout: `run_onchange_` does the work of a master script

A single "Master Bootstrap Script" runs once, and the next time you add a
package you either run all of it again or install by hand, and the list drifts
from the machine. chezmoi gives you better than that for free:

- **`run_once_`** runs once per machine, meaning once per distinct script
  content. Use it for things that are installed and then left alone (rustup,
  nvm, cloning repos).
- **`run_onchange_`** runs again whenever its rendered content changes. Have
  the script embed a hash of the list it installs:

      #!/bin/bash
      # packages/apt.txt hash: {{ include "packages/apt.txt" | sha256sum }}
      set -euo pipefail
      sed 's/#.*//' {{ joinPath .chezmoi.sourceDir "packages/apt.txt" | quote }} \
        | tr -s ' \t' '\n' | grep -v '^!' | grep . \
        | xargs sudo apt-get install -y

  Adding a line to `apt.txt` and running `chezmoi apply` installs exactly that,
  on every machine. The list *is* the install, so it cannot drift. A `!name`
  entry marks a package that is known and deliberately *not* installed
  (installer noise, mostly), so `capture.sh` stops reporting it. Whatever reads
  a list has to skip those, which is what the `grep -v '^!'` is for.
- **`before_` / `after_`** decide whether a script runs before or after the
  files are written. Repos and keyrings go before; anything that reads a
  managed file goes after.
- The number prefixes fix the order: scripts run in alphabetical order within
  each phase.

Every script is idempotent (`apt-get install` of something already installed,
`systemctl enable` of something already enabled) so running it twice costs
nothing. That matters, because a script that fails halfway gets run again.

## Phase 1: capture (on this machine)

Write `capture.sh` in the source repo. It regenerates the raw inventory into
`packages/*.raw`, which is gitignored, so you can diff it against the curated
lists:

    apt-mark showmanual            > packages/apt.raw
    snap list | awk 'NR>1{print $1}' > packages/snap.raw
    code --list-extensions         > packages/vscode.raw
    cargo install --list | grep -E '^[a-z]' | cut -d' ' -f1 > packages/cargo.raw
    dconf dump / > desktop/dconf.raw

**Curate, don't dump.** A raw `apt-mark showmanual` also lists `grub-efi`,
`shim-signed`, `linux-generic`, `ubuntu-minimal` and the installer's language
packs. Putting those into `apt-get install` on a machine with different
hardware or a newer kernel line ranges from pointless to harmful. The curated
list holds only what you chose. From today's 88 that comes to roughly:

    build-essential pkg-config mold libssl-dev libwebkit2gtk-4.1-dev libxdo-dev
    git gh curl neovim tmux kate wmctrl xdotool devilspie2 dsh zram-tools
    ubuntu-desktop xfce4 xfce4-systemload-plugin xfdesktop4 xubuntu-wallpapers
    lightdm lightdm-gtk-greeter
    code docker-ce docker-ce-cli containerd.io docker-buildx-plugin
    docker-compose-plugin tailscale texlive-latex-base libfuse2t64
    python3-netifaces intel-media-va-driver

The webkit/xdo/ssl dev packages are what Dioxus needs to build `fe`. Give them
a comment in `apt.txt` saying so, or they will look like cruft in a year.

Do the same for snaps (`firefox thunderbird chromium htop`) and for `dconf`:
keep only `/org/gnome/desktop/input-sources/`, the ibus keys and
`tiling-assistant`, not the whole dump, which is mostly session state.

`capture.sh` never writes to the curated files. Its job is to show you what
has drifted (`diff packages/apt.txt packages/apt.raw`), not to decide for you.

## Phase 2: dotfiles

    chezmoi init
    chezmoi add ~/.bashrc ~/.profile ~/.tmux.conf ~/.gitconfig
    chezmoi add ~/.config/xfce4/xfconf/xfce-perchannel-xml
    chezmoi add ~/.config/autostart ~/.config/devilspie2/screen_assign.lua
    chezmoi add ~/.config/systemd/user
    chezmoi add ~/.local/bin/{ag,audio-*,xfce-click-show-desktop.sh,…}
    chezmoi add --follow=false ~/.local/bin/rn-*    # the links, not the scripts

Then delete every `*.bak*` from the source directory and list the pattern in
`.chezmoiignore`.

**One umask for everything.** chezmoi records whether a file is executable or
private, and derives every other permission bit from one `umask`. The files
here disagreed: 61 of 70 were group-writable (664/775, Ubuntu's default
umask), and 9 were 644/755. That covers the systemd units, `.bashrc`, `.profile` and
three panel launchers. The config sets `umask = 0o002` to match the majority,
so the first `apply` on this machine makes those 9 group-writable. On a
one-user machine that is cosmetic, and it is the only difference `chezmoi
diff` shows: 0 content lines, checked 2026-09-11.

**xfconf is the one trap here.** `xfconfd` holds the settings in memory and
writes its own copy back over the XML files at logout, so writing the files
under a running XFCE session gets them silently reverted. There are two ways
to handle it:

- On a fresh install, run the bootstrap **before the first XFCE login**: from
  a TTY, or from the default Ubuntu/GNOME session the installer boots into.
  That ordering is the whole fix.
- On a machine where XFCE is already running, the xfconf `run_onchange_`
  script does `pkill -x xfconfd` after apply. It restarts on demand and reads
  the new files. Then `xfce4-panel -r`.

`displays.xml` is tied to specific monitor names, so template it or leave it
out. On different hardware it describes monitors that are not there.

**`~/.local/bin` needs a decision per file.** The `rn-*` scripts are rn
tooling, so they live in rn's `scripts/`, and `~/.local/bin/rn-*` are symlinks
to `~/rn/scripts/`. chezmoi carries only the links. A link points into a
checkout that doesn't exist until `70-projects` clones it, so on a fresh
machine it dangles for the length of one `apply`, which is harmless.
Committing the scripts here instead would give them two sources of truth.

`rtk`, `claude` and the llama.cpp binaries are downloaded, not written, so
they go in `.chezmoiexternal.toml` or in the toolchain script, never
committed.

## Phase 3: secrets

Use chezmoi's built-in **age** encryption. The encrypted files live in the
repo, and the age identity lives outside it: in a password manager, or on
paper, or both.

| Secret | Plan |
|---|---|
| `~/.ssh/id_ed25519` | **Carried as-is**, age-encrypted, so a rebuilt machine is the same identity to anything that trusts this key. That does *not* include GitHub, where the key isn't registered: git to GitHub goes over HTTPS through `gh`. Registering it (`gh ssh-key add`, which needs the `admin:public_key` scope this `gh` login lacks) would be a separate decision. `id_ed25519.pub` and `known_hosts` go in plain, and `authorized_keys` is left out (it's empty). chezmoi's `private_` prefix gives `~/.ssh` 0700 and the key 0600. Get that wrong and ssh refuses the key, which looks like an auth failure rather than a permissions one. The cost of carrying one key: if it leaks, every machine that has it is exposed at once. |
| `rclone.conf` | age-encrypted, as a **`create_`** entry. rclone rewrites the file whenever it refreshes a token, so a normal entry would have `chezmoi apply` put a stale token back over a live one. `create_` writes it only when it is missing, which is the fresh-machine case. The refresh tokens do expire eventually, so the checklist says to run `rclone config reconnect gdrive:` if a mount fails. |
| `~/.config/rn/credentials` | age-encrypted, `create_` for the same reason: rn writes this file itself. `docs/sec.md` is the authority on what is in it. |
| `ANTHROPIC_API_KEY` | Exported on the last line of `.bashrc`, so `.bashrc` is a template (`dot_bashrc.tmpl`) whose line reads `secrets/anthropic_api_key.age` through `decrypt`. The rendered file is byte-identical to the original (checked), so the key is in the repo only as ciphertext. |
| gh, Tailscale, browser sessions | Not captured. Signing in again is the correct behaviour. |

The age identity is the one thing that can't be bootstrapped, since it
decrypts everything else. It lives at `~/.config/chezmoi/key.txt` (mode 600,
never in the repo), and `.chezmoi.toml.tmpl` names that path and carries the
public recipient. On a fresh machine, put the key there before running
`init --apply`. It was generated with `chezmoi age-keygen`: chezmoi's
built-in age does all the encryption, so the `age` binary is optional
rather than a prerequisite. Test that a restore actually decrypts before you
rely on it: an age key you have never used to decrypt is a guess, not a
backup. Every secret was round-tripped when it was added (`chezmoi cat`
against the original, all identical), which checks the key on this machine
and nothing about a copy of it kept somewhere else.

## Phase 4: the bootstrap scripts

**Built 2026-09-11.** All the scripts share one rule, enforced in
`lib/common.sh`: check before acting, and call sudo only for something
actually missing. chezmoi runs every script it hasn't recorded, on this
machine as on a fresh one, so a script that assumes a blank machine would
reinstall things on the first `apply` here.

**How it was tested.** Each script was rendered with `chezmoi
execute-template`, syntax-checked, and then run on this machine with `sudo`,
`pkill`, `xfce4-panel` and `dconf load` replaced by stubs that only record
the call. Ten of the thirteen did nothing. The other three:

- `00` reached sudo for `age`, which isn't installed;
- `40` reached sudo for LightDM's `50-rn.conf`, which is new;
- `50` reloaded the same dconf values.

It found a real bug. `install_file` is called inside `if` and `&&`, where
bash suspends `set -e`, so a refused sudo came back as "unchanged" and the
script printed "installed". A failed install now exits the script.

**What the first real `apply` on this machine does:**

- asks for sudo twice: once to install `age`, once to write `50-rn.conf`,
  which changes nothing visible because XFCE is already your remembered
  session;
- reloads identical dconf values;
- makes the 9 files described under "One umask for everything"
  group-writable;
- stops `xfconfd` and restarts the panel if xfconfd happens to be running
  at that moment;
- prints one note: `~/ca` has a symlinked `node_modules` but no
  `be/runtime`, so it is left alone.

Nothing else changes.

In order:

1. **`00-prereqs`** (once, before): `apt-get install -y git curl gh age`.
   Everything after assumes these.
2. **`10-apt-repos`** (onchange, before): install the docker, tailscale and
   vscode keyrings and source files from `packages/apt-repos/`, then `apt-get
   update`. Hash the directory into the script so a new repo re-runs it.
   Template `resolute` as `{{ .chezmoi.osRelease.versionCodename }}` so the
   next LTS doesn't need an edit.
3. **`20-apt`**, **`21-snap`** (onchange, after): install the curated lists.
   `lightdm` isn't in today's manual list, because it arrived as a
   dependency, but it matters: a fresh Ubuntu Desktop boots into GDM, and this
   machine logs in through LightDM (`/etc/X11/default-display-manager`). So
   `20-apt` preseeds the choice before installing, so that it doesn't stop to
   ask:

       echo "lightdm shared/default-x-display-manager select lightdm" \
         | sudo debconf-set-selections
       sudo DEBIAN_FRONTEND=noninteractive apt-get install -y lightdm …
       echo /usr/sbin/lightdm | sudo tee /etc/X11/default-display-manager

   It also makes XFCE the default session with `user-session=xfce` in
   `/etc/lightdm/lightdm.conf.d/50-rn.conf`. GNOME stays one pick away in
   the greeter. Nothing is removed. Keeping both desktops means GNOME's
   packages and snaps keep updating alongside XFCE's, and that cost was
   accepted.
4. **`30-toolchains`** (once, after): rustup (`-y --no-modify-path`, since
   `.bashrc` is already managed), the `wasm32-unknown-unknown` target, nvm
   0.40.3 into `~/.config/nvm` (`PROFILE=/dev/null`, for the same reason),
   deno, Claude Code, then `sudo usermod -aG docker $USER`.
5. **`31-cargo`**, **`32-vscode`** (onchange): `cargo install --locked` over
   `cargo.txt`, `code --install-extension` over the list.
   - **`dx` is not compiled.** A crate with a line in
     `packages/cargo-prebuilt.txt` is installed from its upstream release
     tarball, with the tarball's sha256 pinned. Compiling `dioxus-cli` took 23
     minutes in the first restore test, and the tarball takes seconds.
   - **The pinned checksum was cross-checked** against the one Dioxus
     publishes beside the tarball, and the binary reports the same commit,
     `57d6794`, as the one compiled here.
   - **A download that doesn't match is refused, and the crate compiles
     after all.** `31-cargo` and `capture.sh` recognise a prebuilt install
     by its `--version`, since `cargo install --list` never hears of it.
   **`33-node-keys`** (onchange, added after the others): Node's release keys
   into the default gpg keyring, which is what rn's `install-node.sh
   --require-sig` and therefore `package.sh` need.
   - **Pinned:** the fingerprints live in `packages/node-release-keys.txt`,
     so what counts as a Node key is decided in the dotfiles repo, not by a
     list on GitHub. The script re-runs when that list changes.
   - **Check-first:** when all eight are present it does nothing, and needs
     no network.
   - **Otherwise it downloads:** each missing key file comes from
     nodejs/release-keys, and is imported only when it holds the fingerprint
     it is named after.
   - **A failed download stops the apply,** rather than recording the step
     as done without the keys.
6. **`40-system-files`** (onchange): `sudo install` the files under
   `system/<set>/` into `/etc`, `daemon-reload` if a unit changed, and enable
   the units installed. That means `audio-amp-guard`, and not `ollama`,
   which was disabled here. **The hardware half is gated on the machine**,
   since the es8336 quirk and the amp guard are specific to this Huawei. It
   is a separate `system/bohb-wax9/` set, installed only when:

       {{ if eq (output "cat" "/sys/class/dmi/id/product_name" | trim) "BOHB-WAX9" }}

   Put the same condition in `.chezmoiignore` so another laptop doesn't get
   `audio-guard` either. The memory note on the speaker-switch race is the
   reason these files exist, so link it from a comment in the script.
7. **`50-dconf`** (onchange): `dconf load / < desktop/dconf.ini`, which
   holds six sections: input sources, ibus, interface, window-button layout,
   the mutter keybindings and tiling-assistant. When there is no session bus,
   as when bootstrapping from a TTY, it wraps the load in `dbus-run-session`.
   **`51-xfconf`** (onchange) stops `xfconfd` if it is running, so that it
   reads the new files rather than writing its copy over them. It turned out
   to be D-Bus-activated and idle-exiting: it wasn't running mid-session when
   tested, so the risk exists only while it is up.
8. **`60-user-units`** (once): `systemctl --user daemon-reload` and enable
   the units. `loginctl enable-linger $USER` so `rn-backend` starts without a
   login. Leave `falkordb` and `n8n-alerts` disabled unless they were enabled
   deliberately. n8n is stopped on purpose and should stay that way.
9. **`70-projects`** (once): clone rn to `~/rn`, create the `ca`/`cb`/`cc`
   worktrees (a missing branch starts at `main`), run `scripts/setup.sh` in
   each tree (bundled runtime, `npm ci`, `.env`), `npm ci` in `fe/`, and
   build the launcher into `~/rn/target/debug/rn`. That exact path is what
   `rn-backend.service` runs, so the build sets `CARGO_TARGET_DIR` itself
   rather than trusting whatever the calling pane exports. Each tree gets its
   **own** `node_modules`: the rn CLAUDE.md says the symlinked sharing was
   never chosen, and a rebuild is the free moment to drop it.
   **A tree is set up only when it has neither `be/runtime` nor
   `be/node_modules`**, which is what a fresh clone looks like. A tree with
   one and not the other was arranged by hand. On this machine that is `~/ca`,
   whose `node_modules` is a symlink into `~/rn`, and `npm ci` there would
   replace the symlink with a real directory. Such a tree gets a note
   instead.
   **Do not run a cold `dx build` here.** A from-scratch wasm build OOMs this
   machine, so leave it for the first `fe/s`, run by hand once the desktop is
   up.
10. **`99-checklist`** (once): print, don't do, the steps that need a human:

        sudo tailscale up                 # gh was signed in before chezmoi ran
        rclone config reconnect gdrive:   # only if the mount fails
        docker login docker.n8n.io        # only if n8n is ever restarted
        sign in: Firefox, Thunderbird, VS Code settings sync
        log out and back in (docker group, xfconf)

## Phase 5: prove it restores

A bootstrap you have never run is a plan, not a backup.

### Run in a container, 2026-09-11

**Why a container, not the VM below.** A faithful VM restore needs 20–30 GB
of disk, and the laptop had 16 GB free. It also had no VM software at all:
`spice-vdagent` being enabled, which this section used to cite, says nothing
about that. A container runs the same bootstrap on a blank Ubuntu 26.04 and
fits in the space.

**How to run it.** `~/.local/share/chezmoi/restore-test/run.sh`. It:

- clones the committed dotfiles into a fresh `ubuntu:26.04` container;
- mounts the age key read-only, and points rn's GitHub URL at `~/rn`, so no
  GitHub login is needed;
- runs every script in chezmoi's own order, then does it all again;
- removes the container, and with it the copy of the key, at the end.

It is guarded:

- the container is capped at 3 GB of RAM, and cargo runs with 2 jobs;
- it will not start with Firefox open;
- a host-side watchdog stops the container below 4 GB free disk or 700 MB
  available memory.

The first run took about 90 minutes: `20-apt`, installing 1,980 packages,
took 53 of them, and building `dioxus-cli` took 23. It used about 11 GB of
disk at peak.

**Results, first pass:**

| Step | Result |
|---|---|
| `00`, `10`, the files | ok. `chezmoi verify`: every managed file matches its source |
| secrets | the ssh key, `rclone.conf` and rn's `credentials` all decrypted at 600, and the `.bashrc` key line rendered |
| `20-apt` | all 37 listed packages installed; LightDM became the default next to GNOME's GDM |
| `30`, `31`, `32`, `33` | rustc 1.98.1 with wasm32, `dx 0.7.10` built from source, the VS Code extensions, all 8 Node release keys |
| `40` | `50-rn.conf` installed. **The laptop gate held:** with a made-up product name, none of the ES8336 audio files appeared |
| `70` | `~/rn` and the `ca`/`cb`/`cc` worktrees, each with its own runtime and `node_modules`, and the launcher built. The backend suite: 503 tests, 0 failing |
| `21-snap`, `60-user-units` | failed, as a container must: no snapd, no systemd user session |

**Second pass:** every script exited as on the first pass, and none
installed anything.

**What the run found, and what was fixed:**

- **`21-snap` failed hard** when snapd didn't answer. Inside a real `chezmoi
  apply`, that would stop every script after it. It now warns and continues,
  and `99-checklist` lists any snaps still missing.
- **`capture.sh` stopped at the same point,** so its report ended before the
  VS Code and cargo sections. It now says "snapd is not answering" and
  carries on.
- **`age` was missing from `apt.txt`,** although `00` installs it, so every
  restored machine would report it as drift. It is listed now.
- **The harness's own bug:** its test step ran without nvm loaded and
  reported "npm: command not found" as a failure. Fixed in the harness.

**Still unproven:**

- snaps actually installing;
- the five user units enabling and linger switching on;
- a real LightDM login into XFCE;
- `50-dconf` against a live session;
- the real one-liner, with `gh auth login` and a clone from GitHub.

That is the VM run below.

### The VM run, still open

It needs about 30 GB free and VM software installed, and neither is true here
yet.

1. A fresh Ubuntu 26.04 VM, installed with the defaults.
2. Before the first XFCE login, run the one-liner, give it the age key, and
   answer the prompts.
3. Check:
   - `chezmoi verify` exits 0.
   - The panel, keyboard shortcuts and window rules look like this machine's.
   - `systemctl --user --failed` is empty. Expect the audio units to be
     *absent*, not failed, because the VM isn't a BOHB-WAX9. That checks the
     gate.
   - `~/rn/scripts/check.sh` passes.
4. Run `chezmoi apply` a second time. It must do nothing. If a `run_once_`
   runs again or a `run_onchange_` fires with no change, the ordering or the
   hashing is wrong.

Re-run this after any change to the scripts, and at least once before each
Ubuntu LTS upgrade.

## Keeping it current

- **Dotfiles:** `chezmoi edit ~/.tmux.conf` instead of editing in place, or
  `chezmoi re-add` after an in-place edit. `chezmoi diff` shows drift both
  ways.
- **Packages:** once a month (or before a reinstall), run `capture.sh` and
  read the three diffs. Anything installed by hand and worth keeping goes into
  the curated list. Anything else gets removed.
- **XFCE:** the XML files change every time you touch a setting, so `chezmoi
  re-add ~/.config/xfce4` after a deliberate change, and commit it with a
  message saying what the change was. The XML diff won't tell you.
