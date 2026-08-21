# rn

Rust project. `be/` is the backend (not scaffolded yet), `fe/` is the Dioxus web
frontend. The frontend was seeded from the RERAG frontend
(https://github.com/PieterdenEngelse/RERAG, `frontend/fro`) — its styling rules
below are carried over verbatim and apply here without exception.

## Build, Test, and Development Commands

```bash
# Frontend CSS build (Tailwind v4 + daisyUI)
cd fe && npm install && npm run css:build

# Frontend live preview (serves on :1790)
cd fe && dx serve --platform web

# Frontend compile check
cd fe && cargo check
```

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

### App chrome colors (from RERAG's header)

| Token | Hex | Use |
|---|---|---|
| Brand / title | `#026B7C` | app title, status-light outline button |
| Active nav link | `#7C2A02` | the nav link for the page you're on |
| Idle nav link | `white` | every other nav link |
| Page background | `bg-gray-900` | app shell and header background |
| Panel background | `bg-gray-800` | tiles, modals, dropdowns |
| daisyUI `primary` | `#0D98BA` | daisyUI-styled controls |
| Checkbox fill | `#1D6B9A` | `.onnx-checkbox` checked state |

The app is **dark-only**. `Layout` adds the `dark` class to `<html>` on mount so
any `dark:` variant from daisyUI still resolves; there is no light theme and no
toggle.

## Form Control Rules (carried from RERAG)

- **Never use the HTML `disabled` attribute** on custom-styled checkboxes or
  daisyUI toggles. Browsers fall back to native user-agent rendering for
  disabled form controls, which silently overrides `appearance: none` /
  `background-color` / `border` — the control reverts to a small gray-on-gray
  box with no border.
- **Avoid `opacity-50` on a wrapper to "gray out" a control.** Opacity
  multiplies through to children, so the brand blue and white checkmark both get
  dimmed and look "wrong" instead of "disabled".

## Coding Conventions

- **Indentation**: 4 spaces everywhere; tabs only in Makefiles
- **Rust naming**: `snake_case` modules/functions/variables, `SCREAMING_SNAKE_CASE` constants, `UpperCamelCase` types
- **Dioxus components**: `UpperCamelCase` components in `src/components/`; pages in `src/pages/`
- Routes live in the `Route` enum in `fe/src/app.rs`; every route sits under `#[layout(Layout)]` so it gets the header

## Collaboration Style

- **Confirmation threshold**: Don't ask for confirmation on small or single-file edits — only ask before major or multi-file changes.
- **No speculative pre-builds**: Don't run `cargo build` just to check for errors after making changes.
