# rn

`be/` is the backend (not scaffolded yet), `fe/` is the Dioxus web frontend, and
`shared/` holds the wire types both ends agree on (not scaffolded yet). The
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
  driver — it just doesn't get to invent the shapes.
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

**Style recipe** (to live in `fe/src/components/info.rs` once the first one is
built — not scaffolded yet):

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
```

`dx` has no config key for the dev-server port — it defaults to `:8080` and the
port comes only from the `--port` flag, which `fe/serve.sh` supplies. Running
`dx serve` bare gives you http://localhost:8080 instead.

`assets/styling/output.css` is generated from `assets/styling/index.css` — never
hand-edit `output.css`. Re-run `npm run css:build` after adding class names that
Tailwind hasn't seen yet (or keep `npm run css:watch` running).

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
- **Generated files are never hand-edited**: `docs/node-parameters.md` and `be/runtime-params.json` come from `be/src/runtime-params.ts` via `npm run params:build`; `be/src/generated/wire.ts` comes from `shared/src/` via `cargo run --bin gen-types`. Add a user-facing runtime setting by adding a registry entry — a test enforces that it carries its info-panel text
- **Spawning processes**: absolute paths and an explicitly constructed environment, every time — see Runtime Rules above

## Collaboration Style

- **Confirmation threshold**: Don't ask for confirmation on small or single-file edits — only ask before major or multi-file changes.
- **No speculative pre-builds**: Don't run `cargo build` just to check for errors after making changes.
