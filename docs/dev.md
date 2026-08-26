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

# Backend run / watch / test
cd be && npm run dev
cd be && npm run start:sealed         # against the bundled runtime — what users get
cd be && npm test && npm run typecheck

# Frontend CSS build (Tailwind v4 + daisyUI)
cd fe && npm install && npm run css:build

# Frontend live preview (serves on :1790 — the user's; see CLAUDE.md)
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

`./scripts/check.sh` is the one to run before committing, and running a subset
instead is the mistake `CLAUDE.md` explains.

---

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
