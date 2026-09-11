# Rebuilding this machine with chezmoi

Plan, written 2026-09-11. Nothing described here exists yet. It's about the
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
a single command:

    sh -c "$(curl -fsLS get.chezmoi.io)" -- init --apply PieterdenEngelse/dotfiles

After that, a handful of steps stay manual and say so, because nobody can do
them for you: signing in to Tailscale, gh, rclone and the browsers.

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
| Toolchains | rustup plus `dioxus-cli`, nvm (24.19, 24.20), bun, deno, dotnet | upstream installers |
| User binaries | `ag`, `rn-*`, `audio-*`, `rtk`, `claude`, llama.cpp, xfce helpers | `~/.local/bin` |
| User units | `ag`, `audio-guard`, `falkordb`, `n8n-alerts`, `rclone-gdrive/onedrive`, `rn-backend`, `rn-grid` | `~/.config/systemd/user/` |
| System units | `audio-amp-guard`, `ollama` | `/etc/systemd/system/` |
| Hardware quirks | `es8336-quirk.conf` (`quirk=0x1b0`), `iwlwifi.conf`, `alsa-base.conf` | `/etc/modprobe.d/` |
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

    .chezmoi.toml.tmpl              # asks hostname/role once; sets age recipient
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
    system/                         # files installed to /etc with sudo
        modprobe.d/es8336-quirk.conf
        modprobe.d/iwlwifi.conf
        systemd/audio-amp-guard.service
        systemd/ollama.service

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

    run_once_before_00-bootstrap-prereqs.sh
    run_onchange_before_10-apt-repos.sh.tmpl
    run_onchange_after_20-apt.sh.tmpl
    run_onchange_after_21-snap.sh.tmpl
    run_once_after_30-toolchains.sh
    run_onchange_after_31-cargo.sh.tmpl
    run_onchange_after_32-vscode.sh.tmpl
    run_onchange_after_40-system-files.sh.tmpl
    run_onchange_after_50-dconf.sh.tmpl
    run_once_after_60-user-units.sh
    run_once_after_70-projects.sh
    run_once_after_99-checklist.sh

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
      grep -v '^\s*#' {{ joinPath .chezmoi.sourceDir "packages/apt.txt" | quote }} \
        | xargs sudo apt-get install -y

  Adding a line to `apt.txt` and running `chezmoi apply` installs exactly that,
  on every machine. The list *is* the install, so it cannot drift.
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
| `~/.ssh/id_ed25519` | **Carried as-is**, age-encrypted, so a rebuilt machine is the same identity to GitHub and to anything that trusts this key. `id_ed25519.pub` and `known_hosts` go in plain, and `authorized_keys` is left out (it's empty). chezmoi's `private_` prefix gives `~/.ssh` 0700 and the key 0600. Get that wrong and ssh refuses the key, which looks like an auth failure rather than a permissions one. The cost of carrying one key: if it leaks, every machine that has it is exposed at once. |
| `rclone.conf` | age-encrypted. The OAuth refresh tokens in it do expire, so the checklist says to run `rclone config reconnect gdrive:` if a mount fails. |
| `~/.config/rn/credentials` | age-encrypted, or re-entered. `docs/sec.md` in rn is the authority on what is in it. |
| gh, Tailscale, browser sessions | Not captured. Signing in again is the correct behaviour. |

The age identity is the one thing that can't be bootstrapped, since it
decrypts everything else. `.chezmoi.toml.tmpl` prompts for its path on
`init`. Test that a restore actually decrypts before you rely on it: an age
key you have never used to decrypt is a guess, not a backup.

## Phase 4: the bootstrap scripts

In order:

1. **`00-bootstrap-prereqs`** (once, before): `apt-get install -y git curl
   age`. Everything after assumes these.
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
   `.bashrc` is already managed), the `wasm32-unknown-unknown` target, nvm,
   bun, deno, then `sudo usermod -aG docker $USER`.
5. **`31-cargo`**, **`32-vscode`** (onchange): `cargo install --locked` over
   `cargo.txt`, `code --install-extension` over the list.
6. **`40-system-files`** (onchange): `sudo install` the files under
   `system/` into `/etc`, `daemon-reload`, and enable `audio-amp-guard` and
   `ollama`. **Gate the hardware half on the machine**, since the es8336 quirk
   and the amp guard are specific to this Huawei:

       {{ if eq (output "cat" "/sys/class/dmi/id/product_name" | trim) "BOHB-WAX9" }}

   Put the same condition in `.chezmoiignore` so another laptop doesn't get
   `audio-guard` either. The memory note on the speaker-switch race is the
   reason these files exist, so link it from a comment in the script.
7. **`50-dconf`** (onchange): `dconf load / < desktop/dconf.ini`.
8. **`60-user-units`** (once): `systemctl --user daemon-reload` and enable
   the units. `loginctl enable-linger $USER` so `rn-backend` starts without a
   login. Leave `falkordb` and `n8n-alerts` disabled unless they were enabled
   deliberately. n8n is stopped on purpose and should stay that way.
9. **`70-projects`** (once): clone rn to `~/rn`, create the `ca`/`cb`/`cc`
   worktrees, run `scripts/install-node.sh` in each, `npm install` in `be/`
   and `fe/`. Give each worktree its **own** `node_modules`: the rn
   CLAUDE.md says the symlinked sharing was never chosen, and a rebuild is the
   free moment to drop it.
   **Do not run a cold `dx build` here.** A from-scratch wasm build OOMs this
   machine, so leave it for the first `fe/s`, run by hand once the desktop is
   up.
10. **`99-checklist`** (once): print, don't do, the steps that need a human:

        sudo tailscale up
        gh auth login
        rclone config reconnect gdrive:   # only if the mount fails
        docker login docker.n8n.io        # only if n8n is ever restarted
        sign in: Firefox, Thunderbird, VS Code settings sync
        log out and back in (docker group, xfconf)

## Phase 5: prove it restores

A bootstrap you have never run is a plan, not a backup. `spice-vdagent` is
already enabled here, so a VM is close at hand:

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
