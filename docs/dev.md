# Commands and reference values

Lookup material moved out of `CLAUDE.md`, which keeps the rules and the reasons.
Nothing here is a rule — if something in this file starts telling you what not
to do, it is in the wrong file.

---

## Commands

```bash
# Backend (Node) setup — idempotent, safe to re-run
./scripts/setup.sh                    # Windows: .\scripts\setup.ps1

# Regenerate the runtime-parameter reference + launcher JSON
cd be && npm run params:build

# Regenerate the TypeScript wire types from the shared crate
cd shared && cargo run --bin gen-types

# This worktree's own pair — each watches the tree it sits in, so an edit is
# live without merging it anywhere. Ports per pane: see CLAUDE.md.
cd be && ./d
cd fe && ./s

# Backend run / watch / test
cd be && npm run dev
cd be && npm run start:sealed         # against the bundled runtime — what users get
cd be && npm test && npm run typecheck

# Frontend CSS build (Tailwind v4 + daisyUI)
cd fe && npm install && npm run css:build

# Frontend live preview (this worktree's port, not always :1790 — see CLAUDE.md)
cd fe && ./serve.sh

# Frontend compile check
cd fe && cargo check

# Everything Rust, from the repo root — fe, launcher and shared are one
# workspace, so these cover all three
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets

# Everything, Rust and Node together — run this before committing
./scripts/check.sh                    # Windows: .\scripts\check.ps1

# Build the installable package, then install it for this user (Linux only).
# The page is a release wasm build: memory-hungry from cold. See
# docs/packaging.md §11 for the flags and what each step does.
./scripts/package.sh                  # → dist/rn  (--no-web, --no-sig, --out DIR)
dist/rn/install.sh                    # → ~/.local/share/rn + rn.service + menu entry
~/.local/share/rn/install.sh --uninstall

# The Windows package is cross-built from here — no Windows machine involved.
# Needs cargo-xwin (cargo install cargo-xwin) and llvm-rc (apt install llvm),
# which compiles rn.exe's version information. The MSI wizard is built on
# Windows with WiX v5: .\scripts\package-msi.ps1 (the release workflow does it).
./scripts/package.sh --target windows # → dist/rn-win

# On the Windows machine itself, in order of least effort:
.\install.ps1 -FromRelease            # download the published package; no toolchain at all
.\scripts\install.ps1 -SkipBuild      # install a package tree you carried over
.\scripts\install.ps1                 # build from the checkout (needs Rust, Node, dx)
.\scripts\install.ps1 -Uninstall

# Publish a release, and install it on a machine that has no toolchains at all.
# The build happens on GitHub (.github/workflows/release.yml), because only a
# GitHub-built release can be code-signed — docs/signing.md. Needs gh, signed in.
./scripts/release.sh --test           # build and test on GitHub, publish nothing
./scripts/release.sh                  # tag v<version> and push; the workflow publishes
./scripts/install.sh --from-release   # download that release and install it
```

`./scripts/check.sh` is the one to run before committing, and running a subset
instead is the mistake `CLAUDE.md` explains.

---

## A scratch backend on another runtime

For checking something that only happens under Bun or Deno — a permission
refusal, a runtime flag, a startup failure — without touching the backend on
:3010 or the real run history.

**Point `HOME` somewhere else.** That is the whole trick, and it is not
optional: the launcher passes `RN_SETTINGS_PATH` through to the child but *not*
`RN_JOB_RUNS_PATH`, `RN_JOB_STATE_PATH` or `RN_HISTORY_PATH` (see the
`allow_var` list in `launcher/src/main.rs`), so a scratch backend started any
other way writes its runs and its cursors into the real `~/.config/rn/`. With
`HOME` moved, every one of those defaults follows it, and so does the
launcher's own view of the settings file.

    S=/tmp/rn-scratch && mkdir -p "$S/.config/rn"
    echo '{"jsRuntime": "deno"}' > "$S/.config/rn/settings.json"

    # The runtime is found relative to the launcher binary: <root>/be/runtime/bin/node
    # for the bundled Node, <root>/be/runtime-deno/bin/deno for this one. Both
    # directories are gitignored, and a symlink into an existing install is enough.
    mkdir -p be/runtime-deno/bin && ln -sfn "$(which deno)" be/runtime-deno/bin/deno

    # --print-env resolves everything and spawns nothing — check the argv first.
    env HOME="$S" BACKEND_PORT=3990 BACKEND_HOOKS_PORT=3991 ./target/debug/rn --print-env
    env HOME="$S" BACKEND_PORT=3990 BACKEND_HOOKS_PORT=3991 ./target/debug/rn

    curl -sS -X POST http://127.0.0.1:3990/api/jobs/<id> -d '{}'
    python3 -m json.tool "$S/.config/rn/job-runs.json"   # the run record
    env HOME="$S" ./target/debug/rn --stop

Two things that will waste time otherwise. `current_exe()` resolves symlinks, so
the launcher must be a real file under the worktree — a symlink to a shared
`CARGO_TARGET_DIR` makes it look upward from the cache directory and report *no
Node runtime found*. And `--print-env` shortens the scratch `HOME` to `~` in its
output like any other path, so `settings ~/.config/rn/settings.json` there is
the scratch file, not the real one.

## Hex equivalents for raw-CSS surfaces

The Tailwind palette values behind the UI Color Rules in `CLAUDE.md`. Use these
when working in a `.css` file, where the class names are not available.

| Tailwind class | Hex | Rule |
|---|---|---|
| `text-gray-300` | `#d1d5db` | preferred for secondary/muted labels |
| `text-gray-400` | `#9ca3af` | minimum readable on dark |
| `text-gray-500` | `#6b7280` | **DO NOT USE for text** (only fully-decorative borders, dividers) |
| `text-gray-600` | `#4b5563` | **DO NOT USE for text** |
| `text-blue-400` | `#60a5fa` | link default color |
| `text-blue-300` | `#93c5fd` | link hover color |

Secondary actions may use cyan `#22d3ee` with `#67e8f9` on hover. The app chrome
palette — brand, nav, backgrounds, checkbox fill — stays in `CLAUDE.md`, because
it is needed while writing components rather than only while writing CSS.

---

## Info button markup

There is no copy of the classes here on purpose. `InfoButton` is one component
in `fe/src/components/info.rs`, it takes `title`, `what`, `why` and `if_wrong`,
and reading it takes less time than checking whether a pasted copy is still
current. A duplicated constant is a constant that goes stale silently — the same
argument as `output.css` and `be/src/generated/wire.ts`.
