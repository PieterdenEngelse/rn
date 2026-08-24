# rn

`be/` is the Node backend, `fe/` is the Dioxus web frontend, and `shared/` is
the Rust crate holding the wire types both ends agree on. The
frontend was seeded from the RERAG frontend
(https://github.com/PieterdenEngelse/RERAG, `frontend/fro`). The styling rules
below apply here without exception.

## What this project is

**rn is an automation project. Node.js is the driver; Rust sits underneath where
it is *reasonable*.**

Node.js owns the automation surface — orchestration, scheduling, integrations,
API glue, anything that benefits from the npm ecosystem and from being edited and
re-run in seconds. Rust is reached for underneath it when there's a concrete
reason: hot paths, parse/transform-heavy work, long-running daemons, strict data
handling, anything where Node is genuinely the wrong tool.

"Reasonable" is the operative word and it cuts both ways:

- **Don't** rewrite something in Rust because Rust is nicer. A 30-line Node
  script that runs once an hour stays a 30-line Node script.
- **Do** drop to Rust when the workload actually calls for it, and keep the
  boundary narrow and boring — a CLI invoked from Node, or a small local service
  with a documented interface. Not a sprawl of FFI.
- Every Rust component should be explainable in one sentence: *what it does, and
  why Node wasn't the right home for it.* If that sentence is hard to write, it
  probably belongs in Node.

## Shared wire types

Every value that crosses a process boundary — Rust component to Node, Node to the
Dioxus frontend — is defined **once**, in the `shared/` crate. Nothing on either
side hand-writes a struct for data that arrived from somewhere else.

`fe` already depends on `serde`, `serde_json`, and `gloo-net`, so the frontend is
built to receive JSON. The failure that invites is silent drift: a field renamed
in `be` reaches the browser as `undefined`, at runtime, in whichever panel
happens to read it first. One definition turns that into a build failure instead.

- **Rust consumers depend on `shared/` directly.** `fe` and every Rust component
  take it as a path dependency. One `struct`, one `#[derive(Serialize,
  Deserialize)]`, both ends agreeing by construction rather than by review.
- **Node consumes generated TypeScript, never hand-written types.** `shared/`
  emits `be/src/generated/wire.ts` and `be` imports from there. Node stays the
  driver — it just doesn't get to invent the shapes. Regenerate with
  `cd be && npm run types:build`, and note that `be/test/generated.test.ts`
  fails if the committed file is stale.
- **Currently covers the jobs surface only.** The ~29 monitor and config types
  in `fe/src/api/wire.rs` are still hand-written on both ends and are the next
  to move. Anything added to the jobs surface goes in `shared/`.
- **`fe` takes it with `default-features = false`**, which switches off the
  `typescript` feature so `ts-rs` never reaches the wasm bundle. Check with
  `cargo tree --target wasm32-unknown-unknown -i ts-rs` — it should find
  nothing.
- **`fe` cannot `impl` a shared type.** The orphan rule applies once a type is
  defined in another crate, so behaviour that hangs off a wire type is a free
  function or an extension trait in `fe` — see `api/history.rs` and
  `trigger_label` in `pages/monitor_jobs.rs`.
- **The Rust definition is the source of truth.** Changing a wire type means
  editing `shared/src/`, regenerating, and committing both in the same change.
  Hand-editing the generated file is a bug, exactly like hand-editing
  `output.css`.
- **Keep it types-only.** Data definitions and their serde attributes; no I/O, no
  business logic, no `dioxus` or `tokio` dependency. `fe` compiles it to wasm, so
  anything heavy lands in the browser bundle.
- **An info panel is the place to say a type is shared.** Where a stat or a
  pipeline step displays backend data, the panel should name where the shape is
  defined — it is how the boundary becomes legible instead of magic.

## Educational by design

**rn is also a learning platform for the person using it.** The user should come
away understanding what the automation is actually doing — not just that it
finished. The guiding principle: **make the invisible visible.** Surface the steps, the inputs, the intermediate state, the numbers,
and the reason a thing happened.

This is a first-class product requirement, not polish added at the end. When
there's a tension between "compact UI" and "the user understands what's going
on", understanding wins.

### Info buttons

The main vehicle for this is the **info button**, and it is used *extensively* —
this is deliberate over-provision, not restraint:

- **Default to adding one.** If a user could plausibly ask "what is this?",
  "where did that number come from?", or "what happens if I change this?", the
  control, stat, or pipeline step gets an info button. Err on the side of too
  many.
- **Explain the mechanism, not the label.** A good info panel says what the thing
  does, *why it matters*, what a sensible value looks like, and what visibly
  changes when it's wrong. "Enables the cache" is a wasted panel; "Keeps parsed
  results in memory so a re-run skips the parse step — expect the second run to
  drop from ~4s to ~200ms; stale results are the trade-off" is the bar.
- **Panels, not tooltips.** An info button opens a modal or an expandable panel
  with room for real prose, a small example, and links onward. A `title=`
  attribute is a supplement, never the explanation itself.
- **Teach the plumbing too.** Where a Rust component is doing the work, the info
  panel is the place to say so, and to say why — it's how the Node/Rust split
  becomes legible instead of mysterious.
- Also worth surfacing the same way: what a step *just did* (counts, durations,
  paths), why an automation was skipped, and what the next run will do.

**Alignment.** Within a board, info buttons line up in one vertical column,
whatever the length of the values beside them. A button that tracks the width of
its own row makes a board look like scattered punctuation and makes the reader
hunt for the next one.

This is handled once, not per page: `PARAM_INPUT_ROW_CLASS` carries `w-full` and
the `.param-row` rule in `fe/assets/styling/index.css` pushes each row's last
child to the right edge. A board sizes to its widest row, every row then fills
that width, so the buttons coincide. Use that row class for any labelled value
or control with an info button and the alignment follows; nothing needs a fixed
width per page.

The exception is a button that belongs to a board or panel *header*, or sits
inline next to a control — those are deliberately beside their subject, not in a
column, and are left alone.

**Style recipe** (lives in `fe/src/components/info.rs`; `InfoButton` takes
`title`, `what`, `why`, `if_wrong`, and renders a job's `JobInfo` and a runtime
parameter's `ParamInfo` alike, since both carry that same shape):

```rust
// Button wrapper
const INFO_BUTTON_CLASS: &str =
    "w-6 h-6 min-w-6 min-h-6 shrink-0 rounded flex items-center justify-center cursor-pointer hover:opacity-80";
const INFO_BUTTON_STYLE: &str = "background-color: #7C2A02; border: 1px solid #7C2A02;"; // Rust brand color

// Icon inside it
const INFO_ICON_SVG_CLASS: &str = "w-5 h-5 text-white";
```

The button sits inline next to the thing it explains, carries a `title` for the
hover case, and toggles a signal that renders the panel (`bg-gray-800`, rounded,
`border border-gray-600`, generous padding).

Setup docs live in `docs/`. Start with `docs/setup-js.md` — the Node driver in
`be/` and the reasoning behind each setting. `docs/packaging.md` covers shipping
this as an installable app. `docs/node-parameters.md` is generated — see below.

## Build, Test, and Development Commands

```bash
# Backend (Node) setup — idempotent, safe to re-run
./scripts/setup.sh                    # Windows: .\scripts\setup.ps1

# Regenerate the runtime-parameter reference + launcher JSON
cd be && npm run params:build

# Regenerate the TypeScript wire types from the shared crate
cd shared && cargo run --bin gen-types

# Backend run / watch / test
cd be && npm run dev
cd be && npm run start:sealed         # against the bundled runtime — what users get
cd be && npm test && npm run typecheck

# Frontend CSS build (Tailwind v4 + daisyUI)
cd fe && npm install && npm run css:build

# Frontend live preview (serves on :1790)
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
```

**Run `./scripts/check.sh` rather than a subset.** The Rust crates and the Node
backend have separate runners, and a subset passing tells you nothing about the
half you did not run — that is not hypothetical, it is how the launcher's suite
went unchecked for a whole session while `cargo test` in `fe/` reported green.
The script runs `cargo test --workspace`, `cargo clippy --workspace`, `npm test`
and `npm run typecheck`, names any step that fails, and exits non-zero.

**The Rust crates are a Cargo workspace.** `fe`, `launcher` and `shared` share
one `Cargo.lock` and one `target/` at the repo root, not per-crate ones. That
exists because `shared` is depended on from more than one place, and separate
lockfiles let each crate resolve a common dependency independently — nothing
would have reported them drifting, which is the failure `shared/` was created
to remove, one level down. Two consequences worth knowing: build output is
`target/`, not `fe/target/` or `launcher/target/`, and `launcher/clippy.toml`
is still honoured from the root — checked, the `Command::new` ban fires either
way.

`dx` has no config key for the dev-server port — it defaults to `:8080` and the
port comes only from the `--port` flag, which `fe/serve.sh` supplies. Running
`dx serve` bare gives you http://localhost:8080 instead.

`assets/styling/output.css` is generated from `assets/styling/index.css` — never
hand-edit `output.css`. Re-run `npm run css:build` after adding class names that
Tailwind hasn't seen yet (or keep `npm run css:watch` running).

## Page width

Pages fill the browser window. The page container is `p-6 w-full space-y-4` —
no `max-w-*`, no `mx-auto`. Boards, tables and metric tiles should use the room
a wide display gives them rather than sitting letterboxed in the middle of it.

The exception is running prose. A line longer than about 90 characters is hard
to read, so a block of explanatory text carries its own `max-w-3xl`; panels of
controls do not. Info panels fill the window outright — `w-full h-full`, no
backdrop inset, no vw/vh fraction, sections stacked in reading order.

## UI Color Rules

- **Minimum readable text on dark tiles**: `text-gray-400` — never use `text-gray-500` or darker for any label or secondary text the user needs to read
- **Preferred for secondary/muted labels**: `text-gray-300`
- **When asked to increase contrast**: shift 2 Tailwind steps toward white (e.g. `text-gray-500` → `text-gray-300`)
- **Links are blue, secondary actions can be cyan**: primary clickable links use `text-blue-400 hover:text-blue-300` (hex `#60a5fa` / `#93c5fd`). Secondary actions — "Reset to default", "Show more", "Edit" — may use cyan `#22d3ee hover:#67e8f9` to visually separate them from primary nav. Never use orange, teal, or other colors.
- These rules apply to all Dioxus components and pages without exception

**Hex equivalents** for any raw-CSS surface — these are the Tailwind palette values for the rules above:

| Tailwind class | Hex | Rule |
|---|---|---|
| `text-gray-300` | `#d1d5db` | preferred for secondary/muted labels |
| `text-gray-400` | `#9ca3af` | minimum readable on dark |
| `text-gray-500` | `#6b7280` | **DO NOT USE for text** (only for fully-decorative borders, dividers) |
| `text-gray-600` | `#4b5563` | **DO NOT USE for text** |
| `text-blue-400` | `#60a5fa` | link default color |
| `text-blue-300` | `#93c5fd` | link hover color |

When working in a raw-CSS file, use these hex values directly. Don't introduce
new gray-500-or-darker text colors — the contrast violation isn't visible until
someone actually reads the screen on a dark display.

### App chrome colors

| Token | Hex | Use |
|---|---|---|
| Brand / title | `#026B7C` | app title, status-light outline button |
| Active nav link | `#7C2A02` | the nav link for the page you're on |
| Info button | `#7C2A02` | fill + border of info-button squares (Rust brand color) |
| Idle nav link | `white` | every other nav link |
| Page background | `bg-gray-900` | app shell background |
| Header background | `bg-gray-700` | header bar — ~20% lighter than the shell |
| Panel background | `bg-gray-800` | tiles, modals, dropdowns |
| daisyUI `primary` | `#0D98BA` | daisyUI-styled controls |
| Checkbox fill | `#1D6B9A` | `.onnx-checkbox` checked state |

The app is **dark-only**. `Layout` adds the `dark` class to `<html>` on mount so
any `dark:` variant from daisyUI still resolves; there is no light theme and no
toggle.

## Browser independence

**Everything built here works the same in every browser.** rn is an installed
app; the browser it renders in is whatever the user's machine hands the
launcher, and that is not a variable this project gets to control. A feature
that only behaves correctly in Chrome is not finished.

- **No browser-specific APIs or prefixed CSS** as the only path to a feature.
  If something needs a vendor prefix, ship the standard property alongside it
  and check the feature still works when neither applies.
- **Never rely on a control's native rendering being identical anywhere.** Form
  controls are the worst offenders — see Form Control Rules below, which exists
  because of exactly this.
- **Where a native element renders differently, the difference must be
  cosmetic, never functional.** A dropdown that decorates a text field is fine:
  a browser that ignores the decoration still leaves a typable field. A control
  whose only affordance is browser-provided is not.
- **A screenshot proves the engine that took it and nothing else.** The
  Chromium command in `## Checking the page` is the one that works here;
  `firefox --headless --screenshot` is installed but produced no file in two
  attempts (90s each, fresh profile included), so a second engine currently
  means opening the page by hand. Where an engine has not been checked, say
  so rather than implying it has.
- The same applies to fonts, scrollbar styling and date/time inputs: assume the
  user's browser draws them its own way, and make sure the page still reads
  correctly when it does.

## Form Control Rules

- **Never use the HTML `disabled` attribute** on custom-styled checkboxes or
  daisyUI toggles. Browsers fall back to native user-agent rendering for
  disabled form controls, which silently overrides `appearance: none` /
  `background-color` / `border` — the control reverts to a small gray-on-gray
  box with no border.
- **Avoid `opacity-50` on a wrapper to "gray out" a control.** Opacity
  multiplies through to children, so the brand blue and white checkmark both get
  dimmed and look "wrong" instead of "disabled".

## Runtime Rules (non-negotiable)

rn ships to users as an installed app carrying **its own Node runtime**. It never
uses whatever Node is on the machine — a user who "has Node" often has one your
app cannot see (nvm and friends are shell-level; a `.desktop` launcher or systemd
unit gets a clean environment and finds nothing).

Two Node binaries can't conflict with each other. Environments can. These four
rules exist because of that distinction — treat them as constraints, not
preferences:

- **Never spawn `node`.** Always the absolute path to the bundled runtime,
  derived from the launcher's own location. A PATH fallback is allowed only in
  development, gated behind an explicit `RN_DEV=1`.
- **Never inherit the environment** for the Node child process. Start from
  `env_clear()` and allowlist what goes in. A user's `NODE_OPTIONS` can stop the
  app booting before your first line of code runs — verified, exit 1. `NODE_OPTIONS`
  is set by us, not by them. This one is enforced, not trusted: all spawning goes
  through the `NodeCommand` wrapper, `clippy.toml` bans `std::process::Command::new`
  everywhere else, and a test poisons `NODE_OPTIONS` to prove the seal holds.
- **Native addons are compiled against the bundled Node version** and shipped
  prebuilt per platform. Prefer N-API; prefer moving the work to a Rust component
  over pulling in an addon at all.
- **`be/.nvmrc` is the single source of truth for the version.** Both
  `scripts/install-node.sh` and its PowerShell twin read it, for the dev runtime
  and the shipped one alike; the launcher logs the runtime version and path at startup and
  surfaces both in the UI. Dev/prod drift is the classic "works on my machine".

The install scripts are one thing in two languages (`scripts/*.sh` and
`scripts/*.ps1`). Change one, change the other in the same commit — a drifted
pair is worse than a single one, because it looks maintained.

Detail, measured sizes, and the build checklist: `docs/packaging.md`.

## Coding Conventions

- **Indentation**: 4 spaces everywhere; tabs only in Makefiles
- **Rust naming**: `snake_case` modules/functions/variables, `SCREAMING_SNAKE_CASE` constants, `UpperCamelCase` types
- **Dioxus components**: `UpperCamelCase` components in `src/components/`; pages in `src/pages/`
- Routes live in the `Route` enum in `fe/src/app.rs`; every route sits under `#[layout(Layout)]` so it gets the header
- **Node/Rust boundary**: a Rust component is invoked from Node over a documented interface (CLI args + JSON on stdout, or a small local HTTP service). Document that interface next to the component; no undocumented FFI. The payload types for that interface live in `shared/` — see Shared wire types
- **Info buttons**: adding a control, a stat, or an automation step means adding its info panel in the same change — not a follow-up task
- **Generated files are never hand-edited**: `docs/node-parameters.md` and `be/runtime-params.json` come from `be/src/runtime-params.ts` via `npm run params:build`; `be/src/generated/wire.ts` comes from `shared/src/` via `npm run types:build` (a test fails if it is stale). Add a user-facing runtime setting by adding a registry entry — a test enforces that it carries its info-panel text
- **Spawning processes**: absolute paths and an explicitly constructed environment, every time — see Runtime Rules above

## Collaboration Style

- **Confirmation threshold**: Don't ask for confirmation on small or single-file edits — only ask before major or multi-file changes.
- **No speculative pre-builds**: Don't run `cargo build` just to check for errors after making changes.

## Checking the page

Look at it. `chromium --headless=new --no-sandbox --disable-gpu --hide-scrollbars
--window-size=1400,2000 --virtual-time-budget=30000 --screenshot=/tmp/p.png
http://localhost:1790/...` writes a PNG that can be read directly, and the user's
own screenshots can be pulled from the clipboard with `xclip -selection clipboard
-t image/png -o > /tmp/p.png`.

Do this for anything visual rather than inferring the page from API payloads. A
route rename, a value wrapping mid-number, a panel heading that contradicts its
contents — none of those show up in the data, and all of them have shipped here
because the data looked right.

**Check whether a change is built by looking for it, not by reading a
timestamp.** Pick a string unique to the change and grep the built wasm:

    grep -qa 'items-stretch flex-1 min-h-0' \
      target/wasm32-unknown-unknown/wasm-dev/fe.wasm

The mtime answers a different question and answers it misleadingly. The dev
server here rebuilds intermittently — a change can sit unbuilt for ten minutes
with no compiler running at all, then be picked up — so a stationary timestamp
distinguishes nothing, and I have twice built a confident wrong theory on one.
Pick a marker long enough to be unique: short strings match the surrounding
prose, and `ceiling` once matched an info panel that already used the word.

The same trap in process checks. `pgrep -f` and `pkill -f` match the shell
command that contains the pattern — including this session's own — which has
produced "the build is served", "the server was killed" and "two compiles are
running", none of them true. Use `pgrep -x`.

When a rebuild is genuinely needed, ask for one. Never start a second `dx serve`
to get it — it writes into the same
target directory as the user's, and has already produced a mismatched js/wasm
pair that rendered a blank page.

## The dev servers are the user's

The frontend dev server on **:1790** belongs to the user, who runs it in their
own terminal with `fe/s`. Do not start it, and do not restart it after killing
something — `dx serve` binds the port exclusively, so a session that starts one
makes `./s` fail with `Address already in use`, and the user cannot tell whose
process took it.

- To check the frontend compiles, run `cargo check` in `fe/`. It needs no port
  and is the answer nearly every time.
- If you genuinely need a running server — a screenshot, reproducing a runtime
  bug — take another port: `./serve.sh --port 1791`. Say which port you took.
- Never run it detached (`setsid`, `nohup`, `disown`). A server that outlives
  your session is one the user cannot see, cannot stop from your transcript,
  and will not think to look for.
- Before concluding a server "died", check `ss -lptn 'sport = :1790'` and
  `ps -o ppid= -p <pid>`. More than one session works in this repo, so the
  process holding a port is often not the one you started.

The backend on **:3010** is different: it is launcher-supervised, and
`./target/debug/rn` with `--stop` and `--status` is the way to manage
it. Restarting it to pick up a registry change is normal and expected.
