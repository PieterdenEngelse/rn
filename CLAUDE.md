# rn

`be/` is the Node backend, `fe/` the Dioxus web frontend, `shared/` the Rust
crate holding the wire types both ends agree on. The frontend was seeded from
the RERAG frontend (https://github.com/PieterdenEngelse/RERAG, `frontend/fro`).
The styling rules below apply here without exception.

Commands live in `docs/dev.md`; the rest of `docs/` is indexed at the end of
this file.

## What this project is

**rn is an automation project. Node.js is the driver; Rust sits underneath where
it is *reasonable*.** Node owns the automation surface — orchestration,
scheduling, integrations, API glue, anything that benefits from npm and from
being edited and re-run in seconds. Rust is reached for underneath it with a
concrete reason: hot paths, parse/transform-heavy work, long-running daemons,
strict data handling.

"Reasonable" cuts both ways:

- **Don't** rewrite something in Rust because Rust is nicer. A 30-line Node
  script that runs once an hour stays a 30-line Node script.
- **Do** drop to Rust when the workload calls for it, and keep the boundary
  narrow and boring — a CLI invoked from Node, or a small local service with a
  documented interface. Not a sprawl of FFI.
- Every Rust component is explainable in one sentence: *what it does, and why
  Node wasn't the right home for it.* If that sentence is hard to write, it
  probably belongs in Node.

## Shared wire types

Every value crossing a process boundary is defined **once**, in `shared/`.
Neither side hand-writes a struct for data that arrived from elsewhere: a field
renamed in `be` would otherwise reach the browser as `undefined`, at runtime, in
whichever panel reads it first. One definition makes that a build failure.

- **Rust consumers path-depend on `shared/` directly** — `fe` and every Rust
  component — so both ends agree by construction, not by review.
- **Node consumes generated TypeScript, never hand-written types.** `shared/`
  emits `be/src/generated/wire.ts`. Regenerate with `cd be && npm run
  types:build`; `be/test/generated.test.ts` fails if the committed file is stale.
- **Covers every surface that crosses the boundary.** Jobs, monitor,
  connection, env, and — since the runtime-parameter and config types moved —
  Config → Runtime as well. `fe/src/api/wire.rs` now defines nothing at all; it
  re-exports. Anything new goes in `shared/`, without exception.
- **A closed set is an enum, not a string.** `ParamKind`, `ParamType`,
  `AppliesAt`, `Category` and `JsRuntime` are enums so the TypeScript stays the
  literal union `be` had before the move — `category: "memry"` is still a build
  failure in a registry of a thousand lines. Their wire spellings are pinned by
  a test in `shared/src/params.rs`: rename a variant without a
  `#[serde(rename)]` and `fe` stops parsing `/api/params` outright, which is a
  blank page rather than one `undefined` field.
- **`fe` takes it `default-features = false`**, switching off the `typescript`
  feature so `ts-rs` never reaches the wasm bundle. Verify with `cargo tree
  --target wasm32-unknown-unknown -i ts-rs` — it should find nothing.
- **`fe` cannot `impl` a shared type** (orphan rule). Behaviour hanging off a
  wire type is a free function or extension trait in `fe` — see `api/history.rs`,
  and `trigger_label` and `runtime_key` in `pages/monitor_jobs.rs` and
  `pages/config.rs`.
- **The Rust definition is the source of truth.** Edit `shared/src/`,
  regenerate, commit both together. Hand-editing the generated file is a bug,
  exactly like hand-editing `output.css`.
- **Types only** — definitions and their serde attributes. No I/O, no business
  logic, no `dioxus` or `tokio`; `fe` compiles this to wasm.
- **An info panel is the place to say a type is shared**, so the boundary is
  legible instead of magic.

## Educational by design

**rn is also a learning platform for the person using it.** The user should come
away understanding what the automation is actually doing, not just that it
finished. **Make the invisible visible**: the steps, the inputs, the
intermediate state, the numbers, and the reason a thing happened. A first-class
product requirement, not polish added at the end — where "compact UI" and "the
user understands what's going on" conflict, understanding wins.

### Info buttons

The main vehicle, used *extensively* — deliberate over-provision, not restraint.

- **Default to adding one.** If a user could plausibly ask "what is this?",
  "where did that number come from?" or "what happens if I change this?", the
  control, stat or pipeline step gets one. Err on the side of too many.
- **Explain the mechanism, not the label** — what it does, *why it matters*,
  what a sensible value looks like, what visibly changes when it is wrong.
  "Enables the cache" is a wasted panel; "Keeps parsed results in memory so a
  re-run skips the parse step — expect the second run to drop from ~4s to
  ~200ms; stale results are the trade-off" is the bar.
- **Panels, not tooltips** — room for real prose, a small example, links onward.
  A `title=` attribute is a supplement, never the explanation itself.
- **Teach the plumbing too.** Where a Rust component does the work, say so and
  say why; it is how the Node/Rust split becomes legible instead of mysterious.
- Surface the same way: what a step *just did* (counts, durations, paths), why
  an automation was skipped, and what the next run will do.

**Alignment.** Info buttons line up in one vertical column within a board,
whatever the length of the values beside them — a button tracking its own row
makes the board look like scattered punctuation. Handled once, not per page:
`PARAM_INPUT_ROW_CLASS` carries `w-full` and the `.param-row` rule in
`fe/assets/styling/index.css` pushes each row's last child to the right edge, so
a board sizes to its widest row and the buttons coincide. Use that row class for
any labelled value or control with an info button; nothing needs a fixed width
per page. The exception is a button belonging to a board or panel *header*, or
sitting inline beside a control — deliberately outside the column, leave it.

`InfoButton` lives in `fe/src/components/info.rs` and takes `title`, `what`,
`why`, `if_wrong`, rendering a job's `JobInfo` and a runtime parameter's
`ParamInfo` alike since both carry that shape. Read that file for the classes
and panel markup rather than copying them here — the copy that used to live in
this file had gone stale against the component.

## Building and checking

Commands are in `docs/dev.md`. Four things there that are not just commands:

**Run `./scripts/check.sh` rather than a subset.** The Rust crates and the Node
backend have separate runners, and a subset passing tells you nothing about the
half you did not run — that is how the launcher's suite went unchecked for a
whole session while `cargo test` in `fe/` reported green. It runs `cargo test
--workspace`, `cargo clippy --workspace`, `npm test` and `npm run typecheck`,
names any step that fails, and exits non-zero.

**The Rust crates are one Cargo workspace.** `fe`, `launcher` and `shared` share
one `Cargo.lock` and one `target/` at the repo root, because separate lockfiles
would let each crate resolve a common dependency independently with nothing
reporting the drift — the failure `shared/` exists to remove, one level down. So
build output is `target/`, not `fe/target/` or `launcher/target/`, and
`launcher/clippy.toml` is still honoured from the root (checked: the
`Command::new` ban fires either way).

**`dx` has no config key for the dev-server port.** It defaults to `:8080` and
the port comes only from `--port`, which `fe/serve.sh` supplies. Bare `dx serve`
gives you http://localhost:8080 instead.

**`assets/styling/output.css` is generated** from `assets/styling/index.css` —
never hand-edit `output.css`. Re-run `npm run css:build` after adding class
names Tailwind hasn't seen yet, or keep `npm run css:watch` running.

**The stylesheet URL is content-hashed, so never fetch a remembered one.**
`dx` does re-bundle a CSS change and serve it within about 20 seconds —
measured, both for a CSS-only edit and for one made alongside a Rust change.
What does not update is the *name*: the hash changes with the content, so an
href copied from an earlier page, or grepped out of a stale wasm, keeps serving
the stylesheet that hash was minted for. That reads as "the rebuild is not
happening" and cost an hour once. Read the href from the live DOM every time:

    href=$(chromium --headless=new --no-sandbox --disable-gpu \
      --virtual-time-budget=8000 --dump-dom http://localhost:PORT/ \
      | grep -o 'href="[^"]*\.css"' | head -1 | sed 's/href="//;s/"//')
    curl -s "http://127.0.0.1:PORT$href" | grep -c 'your-new-class'

It is the same trap as reading `target/` for the wasm: the artifact on disk and
the artifact being served are different questions, and only the second one is
the page.

**A `w-*` utility on a number input does nothing.** `index.css` caps every
`input[type="number"].input-xs` at `max-width: 4rem !important`, which outranks
any Tailwind width — `!w-24`, `!w-32` and `!w-64` all render identically, which
is how `PARAM_NUMBER_INPUT_CLASS` carried an inert width for as long as it
existed. A field that needs more room takes `PARAM_NUMBER_INPUT_WIDE_CLASS`,
whose exception is written beside the cap in `index.css`.

## Page width

Pages fill the browser window. The page container is `p-6 w-full space-y-4` — no
`max-w-*`, no `mx-auto`. Boards, tables and metric tiles should use the room a
wide display gives them rather than sitting letterboxed in the middle of it.

The exception is running prose: a line longer than about 90 characters is hard
to read, so a block of explanatory text carries its own `max-w-3xl` while panels
of controls do not. Info panels fill the window outright — `w-full h-full`, no
backdrop inset, no vw/vh fraction, sections stacked in reading order.

## UI Color Rules

These apply to all Dioxus components and pages without exception.

- **Minimum readable text on dark tiles**: `text-gray-400`. Never
  `text-gray-500` or darker for any label or secondary text the user needs to
  read.
- **Preferred for secondary/muted labels**: `text-gray-300`.
- **"Increase contrast"** means shift 2 Tailwind steps toward white (e.g.
  `text-gray-500` → `text-gray-300`).
- **Links are blue, secondary actions can be cyan.** Primary clickable links use
  `text-blue-400 hover:text-blue-300`. Secondary actions — "Reset to default",
  "Show more", "Edit" — may use cyan `#22d3ee` / `#67e8f9` to separate them from
  primary nav. Never orange, teal, or other colours.

On a raw-CSS surface use the hex equivalents in `docs/dev.md` directly, and
don't introduce new gray-500-or-darker text colours there — the contrast
violation isn't visible until someone actually reads the screen on a dark
display.

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
app and the browser it renders in is whatever the user's machine hands the
launcher — not a variable this project controls. A feature that only behaves
correctly in Chrome is not finished.

- **No browser-specific APIs or prefixed CSS as the only path to a feature.**
  Ship the standard property alongside any prefix, and check the feature still
  works when neither applies.
- **Never rely on a control's native rendering being identical anywhere.** Form
  controls are the worst offenders — see Form Control Rules below, which exists
  because of exactly this.
- **Where a native element renders differently, the difference must be cosmetic,
  never functional.** A dropdown decorating a text field is fine, since a browser
  that ignores the decoration still leaves a typable field; a control whose only
  affordance is browser-provided is not.
- **A screenshot proves the engine that took it and nothing else.** The Chromium
  command under "Checking the page" is the one that works here; `firefox
  --headless --screenshot` is installed but produced no file in two attempts (90s
  each, fresh profile included), so a second engine currently means opening the
  page by hand. Where an engine has not been checked, say so.
- The same applies to fonts, scrollbar styling and date/time inputs: assume the
  browser draws them its own way and make sure the page still reads correctly.

## Form Control Rules

- **Never use the HTML `disabled` attribute** on custom-styled checkboxes or
  daisyUI toggles. Browsers fall back to native user-agent rendering for
  disabled controls, silently overriding `appearance: none` / `background-color`
  / `border` — the control reverts to a small gray-on-gray box with no border.
- **Avoid `opacity-50` on a wrapper to "gray out" a control.** Opacity
  multiplies through to children, so the brand blue and the white checkmark both
  dim and it looks "wrong" rather than "disabled".

## Runtime Rules (non-negotiable)

rn ships as an installed app carrying **its own Node runtime**, and never uses
whatever Node is on the machine — a user who "has Node" often has one the app
cannot see, since nvm and friends are shell-level and a `.desktop` launcher or
systemd unit gets a clean environment. Two Node binaries can't conflict with
each other; environments can. Constraints, not preferences:

- **Never spawn `node`.** Always the absolute path to the bundled runtime,
  derived from the launcher's own location. A PATH fallback is allowed only in
  development, gated behind an explicit `RN_DEV=1`.
- **Never inherit the environment** for the Node child: start from `env_clear()`
  and allowlist what goes in. A user's `NODE_OPTIONS` can stop the app booting
  before your first line of code runs (verified, exit 1), and `NODE_OPTIONS` is
  set by us, not by them. Enforced rather than trusted — all spawning goes
  through the `NodeCommand` wrapper, `clippy.toml` bans
  `std::process::Command::new` everywhere else, and a test poisons
  `NODE_OPTIONS` to prove the seal holds.
- **Native addons are compiled against the bundled Node version** and shipped
  prebuilt per platform. Prefer N-API; prefer moving the work to a Rust
  component over pulling in an addon at all.
- **`be/.nvmrc` is the single source of truth for the version**, read by both
  `scripts/install-node.sh` and its PowerShell twin, for the dev runtime and the
  shipped one alike. The launcher logs the version and path at startup and
  surfaces both in the UI; dev/prod drift is the classic "works on my machine".

The install scripts are one thing in two languages (`scripts/*.sh` and
`scripts/*.ps1`). Change one, change the other in the same commit — a drifted
pair is worse than a single one, because it looks maintained.

Detail, measured sizes and the build checklist: `docs/packaging.md`.

## Coding Conventions

- **Indentation**: 4 spaces everywhere; tabs only in Makefiles
- **Rust naming**: `snake_case` modules/functions/variables,
  `SCREAMING_SNAKE_CASE` constants, `UpperCamelCase` types
- **Dioxus**: `UpperCamelCase` components in `src/components/`, pages in
  `src/pages/`. Routes live in the `Route` enum in `fe/src/app.rs`; every route
  sits under `#[layout(Layout)]` so it gets the header
- **Node/Rust boundary**: a Rust component is invoked from Node over a
  documented interface (CLI args + JSON on stdout, or a small local HTTP
  service). Document it next to the component; no undocumented FFI. Payload
  types live in `shared/` — see Shared wire types
- **Info buttons**: adding a control, a stat or an automation step means adding
  its info panel in the same change, not as a follow-up task
- **Generated files are never hand-edited**: `docs/node-parameters.md` and
  `be/runtime-params.json` come from `be/src/runtime-params.ts` via `npm run
  params:build`; `be/src/generated/wire.ts` comes from `shared/src/` via `npm
  run types:build`, and a test fails if it is stale. Add a user-facing runtime
  setting by adding a registry entry — a test enforces that it carries its
  info-panel text
- **Spawning processes**: absolute paths and an explicitly constructed
  environment, every time — see Runtime Rules

## Collaboration Style

- **Confirmation threshold**: don't ask for confirmation on small or single-file
  edits; only before major or multi-file changes.
- **No speculative pre-builds**: don't run `cargo build` just to check for
  errors after making changes.

## Checking the page

Look at it, for anything visual, rather than inferring the page from API
payloads. A route rename, a value wrapping mid-number, a panel heading that
contradicts its contents — none show up in the data, and all have shipped here
because the data looked right.

    chromium --headless=new --no-sandbox --disable-gpu --hide-scrollbars \
      --window-size=1400,2000 --virtual-time-budget=30000 \
      --screenshot=/tmp/p.png http://localhost:1790/...

The PNG can be read directly. The user's own screenshots come off the clipboard
with `xclip -selection clipboard -t image/png -o > /tmp/p.png`.

**A screenshot saved inside the repository is ignored whatever you call it.**
`.gitignore` covers `*.png`, so a shot dropped anywhere in the tree cannot ride
along on a `git add -A` — which is how `shot.png` was committed once. The two
exceptions are `docs/**` and `fe/assets/**`, where a PNG is kept on purpose; a
screenshot worth keeping goes to `docs/` under a name that says what it shows.

**Check whether a change is built by looking for it, not by reading a
timestamp** — grep the built wasm for a string unique to the change:

    grep -qa 'items-stretch flex-1 min-h-0' \
      target/wasm32-unknown-unknown/wasm-dev/fe.wasm

The dev server rebuilds intermittently, so a change can sit unbuilt for ten
minutes with no compiler running at all and then be picked up; a stationary
mtime distinguishes nothing, and I have twice built a confident wrong theory on
one. Pick a marker long enough to be unique — short strings match the
surrounding prose, and `ceiling` once matched an info panel that already used
the word.

Same trap in process checks: `pgrep -f` and `pkill -f` match the shell command
containing the pattern, including this session's own, which has produced "the
build is served", "the server was killed" and "two compiles are running", none
of them true. Use `pgrep -x`.

When a rebuild is genuinely needed in the tree someone else is serving, ask for
one. Starting a second `dx serve` used to be banned outright, because it wrote
into the same target directory as the user's and had already produced a
mismatched js/wasm pair that rendered a blank page. `fe/serve.sh` now derives
the build directory from the worktree's name, so that collision is gone and a
worktree may serve itself — see The dev servers are the user's below.

## The dev servers are the user's

The frontend dev server on **:1790** belongs to the user, who runs it in their
own terminal with `fe/s` from `~/rn`. Do not start *that* one, and do not
restart it after killing something — `dx serve` binds the port exclusively, so
a second one on the same port cannot serve anyway.

`./s` checks the port before it starts anything, and names what holds it — the
process, its pid, its tty and when it started — rather than letting `dx` fail
with a bare `Address already in use`, which says nothing about whose process
took it. So one port is one server structurally, and a pane showing two status
boxes is one server that has rebuilt twice: `dx` re-emits its panel after every
build, and `Full rebuild: triggered manually` in the log above them means
somebody pressed `r`, once per line.

**A worktree serves itself.** `dx` watches the directory it was started in and
nothing else, so a session editing `~/cb` gets no hot reload from the server
running out of `~/rn` — as far as that watcher is concerned the file never
changed, and pressing `r` rebuilds a tree that has not been touched. That is
not a fault to debug; it is what one watcher on one directory means. `./s`
therefore serves the worktree it sits in, on that pane's own port:

| worktree | port | build directory |
|---|---|---|
| `~/rn` | 1790 | `~/.cache/rn-target` |
| `~/ca` | 1791 | `~/.cache/rn-target-ca` |
| `~/cb` | 1792 | `~/.cache/rn-target-cb` |
| `~/cc` | 1793 | `~/.cache/rn-target-cc` |

**The ports are not `serve.sh`'s doing** — `rn-grid.service` already exports a
`PORT` per pane, and those are the four `RN_CORS_ORIGIN` allows for `localhost`
and `127.0.0.1` alike. `serve.sh` reads it and does not reproduce the mapping;
a second copy could only ever disagree with the first. A fifth worktree needs
a port in the service and in `RN_CORS_ORIGIN`, or its fetches fail CORS while
the page itself looks fine.

The build directory is the part the environment gets wrong, and the only thing
`serve.sh` overrides: that same service exports one `CARGO_TARGET_DIR` into
every pane, so without it each server writes crate `fe` over the others'
output.

- To check the frontend compiles, run `cargo check` in `fe/`. It needs no port
  and is the answer nearly every time.
- If you genuinely need a running server — a screenshot, reproducing a runtime
  bug — run `./s` in your own worktree and say which port that gave you.
- Never run it detached (`setsid`, `nohup`, `disown`). A server that outlives
  your session is one the user cannot see, cannot stop from your transcript, and
  will not think to look for.
- Before concluding a server "died", check `ss -lptn 'sport = :1790'` and `ps -o
  ppid= -p <pid>`. More than one session works in this repo, so the process
  holding a port is often not the one you started.

**A worktree backs itself too.** `be/d` is the twin of `fe/s` (`be/s` is the launcher, and stays that): `node --watch`
on this pane's own port, restarting itself on every save. Between the two, a
change becomes visible in the tree it was made in — which is the whole point,
since the alternative was merging into `~/rn` and restarting its backend to
look at anything.

| worktree | frontend | API | hooks | state |
|---|---|---|---|---|
| `~/rn` | 1790 | 3010 | 3011 | `~/.config/rn` |
| `~/ca` | 1791 | 3020 | 3021 | `~/.cache/rn-state-ca` |
| `~/cb` | 1792 | 3030 | 3031 | `~/.cache/rn-state-cb` |
| `~/cc` | 1793 | 3040 | 3041 | `~/.cache/rn-state-cc` |

Those numbers live in `scripts/dev-ports.sh` and nowhere else — `fe/serve.sh`
and `be/d` both source it, because a frontend pointed at a backend that is not
there renders "backend unreachable", which reads as a broken build rather than
a mismatched pair. They derive from the `PORT` the service exports, so a fifth
worktree needs a port there and in `RN_CORS_ORIGIN`, not an edit here.

`fe` compiles its API address in — a wasm bundle has no environment to read at
runtime — so `serve.sh` passes `RN_API_BASE` to the build. One consequence
worth knowing: `option_env!` is read at compile time and Cargo does not rebuild
when it changes, so if a pane's port ever moves, that worktree needs a clean
build before the frontend believes it.

`~/rn` keeps the real `~/.config/rn`. Every other pane gets its own state, for
the reason each gets its own build directory: four backends sharing one
`job-state.json` would have one worktree's run clear another's cursors, with
nothing in the store to say whose entry it was.

The backend on **:3010** is launcher-supervised — that is the packaged runtime,
not `be/d`, and it does not watch source: manage it with `./target/debug/rn
--stop` and `--status`, or the `be/s` shorthand for the same binary. Restarting it to pick up a registry change is normal and
expected.

**But it can still be orphaned, and has been.** The launcher runs in the
foreground and does not daemonize, so a session that starts it in the background
leaves it running after that session ends — one survived 21 hours that way, with
no tty and a parent that had exited, while `--status` reported only `running (pid
40776)`. `POST /api/restart` does not help: it cycles the *Node child* and leaves
the launcher untouched. `--status` now reports uptime, what started it, and
`ORPHANED` when that parent is gone — check it before assuming a backend on :3010
is one you or the user started, and prefer a tracked background task over a bare
detached start so it shows up in the user's own tooling rather than only in
yours.

## Docs

- `docs/dev.md` — commands, and the hex values for raw-CSS surfaces.
- `docs/setup-js.md` — the Node driver in `be/`, and the reasoning behind each
  setting. Start here.
- `docs/jobs.md` — read before adding an automation: the categories of work, how
  a job gets triggered, why there is one runner with several front doors, what a
  job may remember between runs, and the two places a webhook can be declared.
- `docs/sec.md` — credentials: where they live, what redaction covers, and what
  none of it protects against.
- `docs/token-sec.md` — read before putting a value on a page. Displaying a
  secret is a broadcast rather than a read, which is why a panel reports that a
  value is set and never what it is.
- `docs/network.md` — read before making rn reachable from another machine. The
  API has no authentication, so the bind address is the whole of that security
  position; this is the ordered list of safer answers.
- `docs/tunnel.md` — exposing the hooks listener, and only the hooks listener.
  Includes why Smee is refused: a relay re-serialises the body and every
  signature fails, measured rather than argued.
- `docs/packaging.md` — shipping this as an installable app.
- `docs/n8n.md` — what is worth borrowing from n8n and what is not, with steps.
- `docs/todo.md` — open work, each item saying what goes wrong while it stays
  open.
- `docs/node-parameters.md` — generated; see Coding Conventions.
